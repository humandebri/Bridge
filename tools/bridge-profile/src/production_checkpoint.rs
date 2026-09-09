//! A checkpoint is an approved validation result, not a proof of its history.
//! The registry is deliberately empty until a candidate has been reviewed.
use super::{hex, valid_sha256, LiveRuntimeBinding, ProductionLifecycleView, Profile};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, io::Read, path::Path};

const MAX_BYTES: u64 = 1024 * 1024;
const APPROVED: &[(&str, &str, &str)] = &[];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MigrationInput {
    schema_version: u8,
    gate_b_bundle: std::path::PathBuf,
    seal_receipt: std::path::PathBuf,
    schedule_receipt: std::path::PathBuf,
    execute_receipt: std::path::PathBuf,
    activation_after_receipts: usize,
    receipts: Vec<std::path::PathBuf>,
}

#[derive(Serialize)]
struct AuditArtifact {
    path: std::path::PathBuf,
    sha256: String,
}

#[derive(Serialize)]
struct AuditManifest {
    schema_version: u8,
    kind: &'static str,
    approval: &'static str,
    checkpoint_sha256: String,
    generator_source: Source,
    artifacts: Vec<AuditArtifact>,
}

fn read_receipt(path: &Path) -> Result<(super::ProductionCanisterUpgradeReceipt, String), String> {
    let raw = read_bounded(path, super::MAX_PRODUCTION_UPGRADE_RECEIPT_BYTES as u64)?;
    let receipt =
        serde_json::from_slice(&raw).map_err(|error| format!("{}: {error}", path.display()))?;
    Ok((receipt, hex(&Sha256::digest(&raw))))
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    use std::{io::Write, os::unix::fs::OpenOptionsExt};
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .map_err(|error| error.to_string())?;
    file.write_all(bytes).map_err(|error| error.to_string())?;
    file.sync_all().map_err(|error| error.to_string())
}

fn clean_revision() -> Result<String, String> {
    let status = std::process::Command::new("git")
        .args([
            "status",
            "--porcelain",
            "--untracked-files=all",
            "--ignore-submodules=none",
        ])
        .output()
        .map_err(|error| error.to_string())?;
    if !status.status.success() || !status.stdout.is_empty() {
        return Err("checkpoint candidate generation requires clean committed source".into());
    }
    let revision = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .map_err(|error| error.to_string())?;
    if !revision.status.success() {
        return Err("cannot identify checkpoint generator source".into());
    }
    Ok(String::from_utf8(revision.stdout)
        .map_err(|error| error.to_string())?
        .trim()
        .into())
}

fn file_sha256(path: &Path) -> Result<String, String> {
    let mut file = std::fs::File::open(path).map_err(|error| error.to_string())?;
    let mut hash = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    loop {
        let count = file.read(&mut buffer).map_err(|error| error.to_string())?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Ok(hex(&hash.finalize()))
}

pub(super) fn generate_candidate(
    input_path: &Path,
    output: &Path,
    audit_path: &Path,
) -> Result<(), String> {
    let generator_revision = clean_revision()?;
    let input_bytes = read_bounded(input_path, MAX_BYTES)?;
    let input: MigrationInput =
        serde_json::from_slice(&input_bytes).map_err(|error| error.to_string())?;
    if input.schema_version != 1
        || input.receipts.is_empty()
        || input.activation_after_receipts >= input.receipts.len()
        || input.receipts.iter().any(|path| !path.is_absolute())
        || !input.gate_b_bundle.is_absolute()
        || !input.seal_receipt.is_absolute()
        || !input.schedule_receipt.is_absolute()
        || !input.execute_receipt.is_absolute()
        || output.exists()
        || audit_path.exists()
        || output == audit_path
    {
        return Err("invalid checkpoint migration input or existing output".into());
    }
    let manifest_path = input.gate_b_bundle.join("release-manifest.json");
    let manifest: super::ReleaseManifest = super::read_json(&manifest_path)?;
    let mut inputs = vec![
        input_path.to_path_buf(),
        manifest_path,
        input.seal_receipt.clone(),
        input.schedule_receipt.clone(),
        input.execute_receipt.clone(),
    ];
    for artifact in &manifest.artifacts {
        inputs.push(super::safe_artifact_path(
            &input.gate_b_bundle,
            &artifact.path,
        )?);
    }
    let snapshots = inputs
        .into_iter()
        .map(|path| {
            Ok(AuditArtifact {
                sha256: file_sha256(&path)?,
                path,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let (bundle, gate_a, execute) = super::validate_production_handover_evidence_files(
        &input.gate_b_bundle,
        &input.seal_receipt,
        &input.schedule_receipt,
        &input.execute_receipt,
        super::SealReceiptLiveContext::HistoricalCheckpoint,
    )?;
    let gate_a_profile: Profile = super::read_json(&bundle.root.join("gate-a-profile.json"))?;
    let (first_active, _) = read_receipt(&input.receipts[input.activation_after_receipts])?;
    let mut continuation =
        super::production_ui_upgrade_continuation(&bundle, &gate_a, &execute, &first_active)?;
    continuation.prefix_len = input.activation_after_receipts;
    drop(first_active);
    let (last, _) = read_receipt(input.receipts.last().ok_or("empty migration history")?)?;
    let module = last.after_module_sha256.clone();
    let verified_at = last.verified_at_unix;
    drop(last);
    let mut source = Source {
        revision: gate_a.source_revision.clone(),
        tree_sha256: gate_a.source_tree_sha256.clone(),
    };
    verify_source(&source, None)?;
    let mut artifacts = Vec::new();
    let mut receipts = Vec::new();
    let mut installs = Vec::new();
    let entries = input.receipts.iter().map(|path| {
        let (receipt, digest) = read_receipt(path)?;
        let next = Source {
            revision: receipt.source_revision.clone(),
            tree_sha256: receipt.source_tree_sha256.clone(),
        };
        verify_source(&next, Some(&source.revision))?;
        source = next;
        let submission: super::ProductionUpgradeSubmission =
            serde_json::from_slice(&super::decode_hex(&receipt.submission_json_hex)?)
                .map_err(|error| error.to_string())?;
        installs.push(Install {
            request_id: submission.request_id,
            signed_install_sha256: submission.signed_update_sha256,
        });
        receipts.push(digest.clone());
        artifacts.push(AuditArtifact {
            path: path.clone(),
            sha256: digest,
        });
        Ok(receipt)
    });
    let terminal = super::validate_production_upgrade_receipts(
        &gate_a_profile,
        &gate_a,
        entries,
        &module,
        super::CURRENT_STABLE_SCHEMA_VERSION,
        Some(&continuation),
    )?;
    let mut record_artifact = |path: &Path| -> Result<String, String> {
        let digest = hex(&Sha256::digest(
            std::fs::read(path).map_err(|error| error.to_string())?,
        ));
        artifacts.push(AuditArtifact {
            path: path.to_path_buf(),
            sha256: digest.clone(),
        });
        Ok(digest)
    };
    let gate_a_sha256 = record_artifact(&bundle.root.join("gate-a-receipt.json"))?;
    record_artifact(&bundle.root.join("release-manifest.json"))?;
    let gate_b_sha256 = bundle.manifest_sha256.clone();
    let seal_sha256 = record_artifact(&input.seal_receipt)?;
    let schedule_sha256 = record_artifact(&input.schedule_receipt)?;
    let execute_sha256 = record_artifact(&input.execute_receipt)?;
    record_artifact(input_path)?;
    let checkpoint = Checkpoint {
        schema_version: 1,
        kind: "production-upgrade-checkpoint".into(),
        canister: bundle.profile.bridge_canister_id.clone(),
        network: bundle.profile.ic_host.clone(),
        controller: gate_a.canister_install.installer_principal.clone(),
        deployment_instance_id: terminal.runtime.deployment_instance_id.clone(),
        module_sha256: module,
        runtime: terminal.runtime,
        lifecycle: terminal.lifecycle,
        deposits_paused: terminal.deposits_paused,
        mint_authorization_epoch: terminal.observed_epoch.0,
        mint_authorization_ttl_seconds: terminal.observed_epoch.1,
        roots: Roots {
            gate_a_sha256,
            gate_b_sha256,
            seal_sha256,
            schedule_sha256,
            execute_sha256,
            gate_b_created_at_unix: bundle.manifest.created_at_unix,
            deployment_block_number: gate_a
                .bridge_deployment_block_number
                .max(gate_a.timelock_deployment_block_number),
            gate_b_profile: bundle.profile,
            activation: Activation {
                governance_operation_id: execute
                    .governance_operation_id
                    .parse()
                    .map_err(|_| "invalid activation operation ID")?,
                finalized_block_number: execute
                    .finalized_block_number
                    .parse()
                    .map_err(|_| "invalid activation block")?,
                timelock_operation_id: execute.timelock_operation_id,
                transaction_hash: execute.transaction_hash,
                confirmed_generation: execute.confirmed_generation,
                confirmed_signed_at_ns: execute.confirmed_signed_at_ns,
            },
        },
        source,
        receipt_sha256: receipts,
        installs,
        verified_at_unix: verified_at,
    };
    let bytes = super::canonical_bytes(&checkpoint)?;
    parse(&bytes)?;
    let digest = hex(&Sha256::digest(&bytes));
    let revision = clean_revision()?;
    if revision != generator_revision {
        return Err("checkpoint generator source changed during validation".into());
    }
    for artifact in &snapshots {
        if !artifacts.iter().any(|item| item.path == artifact.path) {
            artifacts.push(AuditArtifact {
                path: artifact.path.clone(),
                sha256: artifact.sha256.clone(),
            });
        }
    }
    for artifact in artifacts.iter().chain(&snapshots) {
        if file_sha256(&artifact.path)? != artifact.sha256 {
            return Err(format!(
                "checkpoint migration input changed: {}",
                artifact.path.display()
            ));
        }
    }
    let archive = std::process::Command::new("git")
        .args(["archive", "--format=tar", &revision])
        .output()
        .map_err(|error| error.to_string())?;
    if !archive.status.success() {
        return Err("cannot hash checkpoint generator source".into());
    }
    let audit = AuditManifest {
        schema_version: 1,
        kind: "production-upgrade-checkpoint-audit",
        approval: "unapproved",
        checkpoint_sha256: digest.clone(),
        generator_source: Source {
            revision,
            tree_sha256: hex(&Sha256::digest(&archive.stdout)),
        },
        artifacts,
    };
    write_new(audit_path, &super::canonical_bytes(&audit)?)?;
    write_new(output, &bytes)?;
    println!("unapproved_checkpoint_sha256={digest}");
    Ok(())
}

pub(super) fn rotate_candidate(
    evidence_path: &Path,
    output: &Path,
    audit_path: &Path,
) -> Result<(), String> {
    let revision = clean_revision()?;
    if output.exists() || audit_path.exists() || output == audit_path {
        return Err("checkpoint rotation outputs must be new distinct files".into());
    }
    let verified = read_evidence(evidence_path)?;
    let bytes = read_bounded(
        evidence_path,
        2 * super::MAX_PRODUCTION_UPGRADE_CHAIN_BYTES as u64 + 4 * MAX_BYTES,
    )?;
    if hex(&Sha256::digest(&bytes)) != verified.evidence_sha256 {
        return Err("checkpoint rotation input changed".into());
    }
    let evidence: Evidence = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    if evidence.entries.is_empty() {
        return Err("checkpoint rotation requires additional verified receipts".into());
    }
    let mut checkpoint = verified.checkpoint;
    checkpoint.module_sha256 = verified.module_sha256;
    checkpoint.runtime = verified.terminal.runtime;
    checkpoint.lifecycle = verified.terminal.lifecycle;
    checkpoint.deposits_paused = verified.terminal.deposits_paused;
    checkpoint.mint_authorization_epoch = verified.terminal.observed_epoch.0;
    checkpoint.mint_authorization_ttl_seconds = verified.terminal.observed_epoch.1;
    checkpoint.source = verified.source;
    for entry in evidence.entries {
        let receipt: super::ProductionCanisterUpgradeReceipt =
            serde_json::from_slice(&super::decode_hex(&entry.receipt_json_hex)?)
                .map_err(|error| error.to_string())?;
        let submission: super::ProductionUpgradeSubmission =
            serde_json::from_slice(&super::decode_hex(&receipt.submission_json_hex)?)
                .map_err(|error| error.to_string())?;
        checkpoint.receipt_sha256.push(entry.receipt_sha256);
        checkpoint.installs.push(Install {
            request_id: submission.request_id,
            signed_install_sha256: submission.signed_update_sha256,
        });
        checkpoint.verified_at_unix = receipt.verified_at_unix;
    }
    let candidate_bytes = super::canonical_bytes(&checkpoint)?;
    parse(&candidate_bytes)?;
    if clean_revision()? != revision || file_sha256(evidence_path)? != verified.evidence_sha256 {
        return Err("checkpoint rotation inputs changed".into());
    }
    let archive = std::process::Command::new("git")
        .args(["archive", "--format=tar", &revision])
        .output()
        .map_err(|error| error.to_string())?;
    if !archive.status.success() {
        return Err("cannot hash checkpoint generator source".into());
    }
    let digest = hex(&Sha256::digest(&candidate_bytes));
    let audit = AuditManifest {
        schema_version: 1,
        kind: "production-upgrade-checkpoint-rotation-audit",
        approval: "unapproved",
        checkpoint_sha256: digest.clone(),
        generator_source: Source {
            revision,
            tree_sha256: hex(&Sha256::digest(&archive.stdout)),
        },
        artifacts: vec![AuditArtifact {
            path: evidence_path.to_path_buf(),
            sha256: verified.evidence_sha256,
        }],
    };
    write_new(audit_path, &super::canonical_bytes(&audit)?)?;
    write_new(output, &candidate_bytes)?;
    println!("unapproved_checkpoint_sha256={digest}");
    Ok(())
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Evidence {
    schema_version: u8,
    kind: String,
    checkpoint_json_hex: String,
    checkpoint_sha256: String,
    entries: Vec<super::ProductionCanisterUpgradeChainEntry>,
}

fn prefix_digest(evidence: &Evidence, length: usize) -> Result<String, String> {
    // Alphabetical field order matches canonical_bytes without copying Wasm.
    #[derive(Serialize)]
    struct Entry<'a> {
        previous_receipt_sha256: &'a Option<String>,
        receipt_json_hex: &'a str,
        receipt_sha256: &'a str,
        sequence: u8,
    }
    #[derive(Serialize)]
    struct Prefix<'a> {
        checkpoint_json_hex: &'a str,
        checkpoint_sha256: &'a str,
        entries: Vec<Entry<'a>>,
        kind: &'a str,
        schema_version: u8,
    }
    struct HashWriter(Sha256);
    impl std::io::Write for HashWriter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.update(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let prefix = Prefix {
        checkpoint_json_hex: &evidence.checkpoint_json_hex,
        checkpoint_sha256: &evidence.checkpoint_sha256,
        entries: evidence.entries[..length]
            .iter()
            .map(|entry| Entry {
                previous_receipt_sha256: &entry.previous_receipt_sha256,
                receipt_json_hex: &entry.receipt_json_hex,
                receipt_sha256: &entry.receipt_sha256,
                sequence: entry.sequence,
            })
            .collect(),
        kind: &evidence.kind,
        schema_version: evidence.schema_version,
    };
    let mut writer = HashWriter(Sha256::new());
    serde_json::to_writer(&mut writer, &prefix).map_err(|error| error.to_string())?;
    writer.0.update(b"\n");
    Ok(hex(&writer.0.finalize()))
}

pub(super) struct VerifiedEvidence {
    pub checkpoint: Checkpoint,
    pub terminal: super::ProductionUpgradeTerminal,
    pub module_sha256: String,
    pub source: Source,
    pub evidence_sha256: String,
}

fn read_bounded(path: &Path, maximum: u64) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(|error| format!("{}: {error}", path.display()))?
        .take(maximum + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() as u64 > maximum {
        return Err(format!("{} exceeds its size limit", path.display()));
    }
    Ok(bytes)
}

fn verify_source(source: &Source, predecessor: Option<&str>) -> Result<(), String> {
    use std::process::{Command, Stdio};
    if source.revision.len() != 40
        || !source.revision.bytes().all(|byte| byte.is_ascii_hexdigit())
        || !lowercase_digest(&source.tree_sha256)
    {
        return Err("invalid checkpoint history source identity".into());
    }
    if let Some(previous) = predecessor {
        if !Command::new("git")
            .args(["merge-base", "--is-ancestor", previous, &source.revision])
            .status()
            .map_err(|error| error.to_string())?
            .success()
        {
            return Err("checkpoint history source ancestry is discontinuous".into());
        }
    }
    let mut child = Command::new("git")
        .args(["archive", "--format=tar", &source.revision])
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|error| error.to_string())?;
    let mut stdout = child.stdout.take().ok_or("missing Git archive output")?;
    let mut hash = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    let read_result = (|| {
        loop {
            let size = stdout
                .read(&mut buffer)
                .map_err(|error| error.to_string())?;
            if size == 0 {
                break;
            }
            hash.update(&buffer[..size]);
        }
        Ok::<_, String>(())
    })();
    drop(stdout);
    let status = child.wait().map_err(|error| error.to_string())?;
    read_result?;
    if !status.success() || hex(&hash.finalize()) != source.tree_sha256 {
        return Err("checkpoint history source tree digest differs".into());
    }
    Ok(())
}

pub(super) fn read_evidence(path: &Path) -> Result<VerifiedEvidence, String> {
    let bytes = read_bounded(
        path,
        2 * super::MAX_PRODUCTION_UPGRADE_CHAIN_BYTES as u64 + 4 * MAX_BYTES,
    )?;
    verify_evidence(&bytes, APPROVED, verify_source)
}

fn ui_runtime(verified: &VerifiedEvidence, rpc_bytes: &[u8]) -> Result<Vec<u8>, String> {
    let roots = &verified.checkpoint.roots;
    super::canonical_bytes(&super::production_ui_runtime_profile_from_digest(
        &roots.gate_b_profile,
        &super::canonical_bytes(&roots.gate_b_profile)?,
        &roots.gate_b_sha256,
        &verified.evidence_sha256,
        &verified.module_sha256,
        &verified.terminal,
        rpc_bytes,
    )?)
}

fn validate_predecessor(verified: &VerifiedEvidence, raw: &[String]) -> Result<String, String> {
    if raw.len() != 5 {
        return Err("checkpoint predecessor needs five query responses".into());
    }
    let (status, lifecycle, runtime, digest) =
        super::production_upgrade_query_state_any(&raw[0], &raw[1], &raw[2], &raw[3])?;
    let operational = super::OperationalEpochEvidence::from_response(&raw[4])?;
    if !super::production_upgrade_predecessor_with_epoch_evidence(
        &verified.terminal,
        &status,
        lifecycle,
        &super::live_runtime_binding_from_view(&runtime),
        Some(&operational),
        verified
            .checkpoint
            .roots
            .gate_b_profile
            .parameters
            .ledger_fee,
    )? {
        return Err(
            "live predecessor differs from the approved checkpoint evidence terminal".into(),
        );
    }
    Ok(digest)
}

pub(super) fn predecessor(path: &Path, module: &str, raw: &[String]) -> Result<(), String> {
    let verified = read_evidence(path)?;
    if module != verified.module_sha256 {
        return Err("live predecessor module differs".into());
    }
    validate_predecessor(&verified, raw)?;
    println!("checkpoint_predecessor=verified");
    Ok(())
}

pub(super) fn preserved(path: &Path, raw: &[String]) -> Result<(), String> {
    if raw.len() != 9 {
        return Err("checkpoint state comparison needs nine query responses".into());
    }
    let verified = read_evidence(path)?;
    let before_digest = validate_predecessor(&verified, &raw[..5])?;
    let (before, _, before_runtime, _) =
        super::production_upgrade_query_state_any(&raw[0], &raw[1], &raw[2], &raw[3])?;
    let (after, _, after_runtime, after_digest) =
        super::production_upgrade_query_state_any(&raw[5], &raw[6], &raw[7], &raw[8])?;
    if before_runtime.schema_version != super::CURRENT_STABLE_SCHEMA_VERSION
        || after_runtime.schema_version != super::CURRENT_STABLE_SCHEMA_VERSION
        || !super::production_upgrade_status_preserved(&before, &after)
        || raw[1] != raw[6]
        || raw[2] != raw[7]
        || raw[3] != raw[8]
        || before_digest != after_digest
    {
        return Err("checkpoint upgrade does not preserve the v36 public state".into());
    }
    println!("{after_digest}");
    Ok(())
}

pub(super) fn render_ui(
    evidence_path: &Path,
    rpc_path: &Path,
    output: &Path,
) -> Result<(), String> {
    let evidence = read_evidence(evidence_path)?;
    write_new(
        output,
        &ui_runtime(&evidence, &read_bounded(rpc_path, MAX_BYTES)?)?,
    )
}

pub(super) fn verify_ui(
    evidence_path: &Path,
    rpc_path: &Path,
    runtime_path: &Path,
) -> Result<(), String> {
    let verified = read_evidence(evidence_path)?;
    let rpc_bytes = read_bounded(rpc_path, MAX_BYTES)?;
    let runtime_bytes = read_bounded(runtime_path, MAX_BYTES)?;
    if ui_runtime(&verified, &rpc_bytes)? != runtime_bytes {
        return Err(
            "UI runtime differs from checkpoint evidence and reviewed RPC rendering".into(),
        );
    }
    let checkpoint = &verified.checkpoint;
    let roots = &checkpoint.roots;
    let activation = super::ProductionHandoverActivationBinding {
        governance_operation_id: roots.activation.governance_operation_id,
        finalized_block_number: roots.activation.finalized_block_number,
        timelock_operation_id: &roots.activation.timelock_operation_id,
        transaction_hash: &roots.activation.transaction_hash,
        confirmed_generation: roots.activation.confirmed_generation,
        confirmed_signed_at_ns: &roots.activation.confirmed_signed_at_ns,
        expected_module_sha256: &verified.module_sha256,
    };
    super::verify_production_live_state(
        &roots.gate_b_profile,
        candid::Principal::from_text(&checkpoint.controller).map_err(|error| error.to_string())?,
        &activation,
        Some(&verified.terminal),
        roots.gate_b_created_at_unix,
        roots.deployment_block_number,
    )?;
    if read_bounded(rpc_path, MAX_BYTES)? != rpc_bytes
        || read_bounded(runtime_path, MAX_BYTES)? != runtime_bytes
        || hex(&Sha256::digest(read_bounded(
            evidence_path,
            2 * super::MAX_PRODUCTION_UPGRADE_CHAIN_BYTES as u64 + 4 * MAX_BYTES,
        )?)) != verified.evidence_sha256
    {
        return Err("production UI inputs changed during live validation".into());
    }
    println!(
        "production_ui=live-pass schema=36 activation=execute manifest_sha256={}",
        roots.gate_b_sha256
    );
    Ok(())
}

pub(super) fn make_evidence(
    checkpoint_path: &Path,
    receipt_paths: &[String],
    output: &Path,
) -> Result<(), String> {
    if receipt_paths.len() > 16 {
        return Err("checkpoint suffix exceeds 16 receipts".into());
    }
    let (checkpoint, digest) = read_approved(checkpoint_path)?;
    let raw = read_bounded(checkpoint_path, MAX_BYTES)?;
    if hex(&Sha256::digest(&raw)) != digest {
        return Err("checkpoint changed while freezing evidence".into());
    }
    let mut previous = checkpoint.receipt_sha256.last().cloned();
    let mut total = 0usize;
    let mut entries = Vec::new();
    for (index, path) in receipt_paths.iter().enumerate() {
        let raw = read_bounded(
            Path::new(path),
            super::MAX_PRODUCTION_UPGRADE_RECEIPT_BYTES as u64,
        )?;
        total = total
            .checked_add(raw.len())
            .filter(|size| *size <= super::MAX_PRODUCTION_UPGRADE_CHAIN_BYTES)
            .ok_or("checkpoint suffix exceeds 256 MiB")?;
        let digest = hex(&Sha256::digest(&raw));
        entries.push(super::ProductionCanisterUpgradeChainEntry {
            sequence: index as u8,
            previous_receipt_sha256: previous,
            receipt_sha256: digest.clone(),
            receipt_json_hex: hex(&raw),
        });
        previous = Some(digest);
    }
    let evidence = Evidence {
        schema_version: 1,
        kind: "production-upgrade-checkpoint-evidence".into(),
        checkpoint_json_hex: hex(&raw),
        checkpoint_sha256: digest,
        entries,
    };
    let bytes = super::canonical_bytes(&evidence)?;
    verify_evidence(&bytes, APPROVED, verify_source)?;
    write_new(output, &bytes)
}

fn verify_evidence(
    bytes: &[u8],
    registry: &[(&str, &str, &str)],
    verify_source: impl Fn(&Source, Option<&str>) -> Result<(), String>,
) -> Result<VerifiedEvidence, String> {
    let evidence: Evidence = serde_json::from_slice(bytes).map_err(|error| error.to_string())?;
    if evidence.schema_version != 1
        || evidence.kind != "production-upgrade-checkpoint-evidence"
        || evidence.entries.len() > 16
        || evidence.checkpoint_json_hex.len() as u64 > 2 * MAX_BYTES
    {
        return Err("invalid checkpoint evidence envelope".into());
    }
    let checkpoint_bytes = super::decode_hex(&evidence.checkpoint_json_hex)?;
    let checkpoint = parse(&checkpoint_bytes)?;
    let digest = hex(&Sha256::digest(&checkpoint_bytes));
    if digest != evidence.checkpoint_sha256 {
        return Err("checkpoint evidence digest differs".into());
    }
    validate_approval(&checkpoint, &digest, registry)?;
    let mut encoded_total = 0usize;
    for entry in &evidence.entries {
        if entry.receipt_json_hex.len() > 2 * super::MAX_PRODUCTION_UPGRADE_RECEIPT_BYTES {
            return Err("checkpoint suffix receipt is too large".into());
        }
        encoded_total = encoded_total
            .checked_add(entry.receipt_json_hex.len())
            .filter(|size| *size <= 2 * super::MAX_PRODUCTION_UPGRADE_CHAIN_BYTES)
            .ok_or("checkpoint suffix is too large")?;
    }
    if prefix_digest(&evidence, evidence.entries.len())? != hex(&Sha256::digest(bytes)) {
        return Err("checkpoint evidence must use the canonical frozen encoding".into());
    }
    let prefix_digests = (0..evidence.entries.len())
        .map(|length| prefix_digest(&evidence, length))
        .collect::<Result<Vec<_>, _>>()?;
    verify_source(&checkpoint.source, None)?;
    let start = super::ProductionUpgradeStart {
        installer: checkpoint.controller.clone(),
        module_sha256: checkpoint.module_sha256.clone(),
        terminal: checkpoint.terminal(),
        minimum_executed_at_unix: checkpoint.verified_at_unix,
        install_request_ids: checkpoint
            .installs
            .iter()
            .map(|item| item.request_id.clone())
            .collect(),
        signed_install_updates: checkpoint
            .installs
            .iter()
            .map(|item| item.signed_install_sha256.clone())
            .collect(),
    };
    let mut source = checkpoint.source.clone();
    let mut module = checkpoint.module_sha256.clone();
    let mut previous = checkpoint.receipt_sha256.last().cloned();
    let mut total = 0usize;
    let expected_module = evidence
        .entries
        .last()
        .map(|entry| {
            if entry.receipt_json_hex.len() > 2 * super::MAX_PRODUCTION_UPGRADE_RECEIPT_BYTES {
                return Err("checkpoint suffix receipt is too large".to_string());
            }
            let receipt: super::ProductionCanisterUpgradeReceipt =
                serde_json::from_slice(&super::decode_hex(&entry.receipt_json_hex)?)
                    .map_err(|error| error.to_string())?;
            Ok(receipt.after_module_sha256)
        })
        .transpose()?;
    // Decode only one receipt while iterating. The bounded envelope retains hex
    // strings but never a second collection of decoded Wasm-bearing receipts.
    let entries = evidence
        .entries
        .into_iter()
        .enumerate()
        .map(|(index, entry)| {
            if entry.receipt_json_hex.len() > 2 * super::MAX_PRODUCTION_UPGRADE_RECEIPT_BYTES {
                return Err("checkpoint suffix receipt is too large".into());
            }
            let raw = super::decode_hex(&entry.receipt_json_hex)?;
            total = total
                .checked_add(raw.len())
                .filter(|size| *size <= super::MAX_PRODUCTION_UPGRADE_CHAIN_BYTES)
                .ok_or("checkpoint suffix is too large")?;
            let digest = hex(&Sha256::digest(&raw));
            if usize::from(entry.sequence) != index
                || entry.previous_receipt_sha256 != previous
                || entry.receipt_sha256 != digest
            {
                return Err("checkpoint suffix linkage is invalid".into());
            }
            let receipt: super::ProductionCanisterUpgradeReceipt =
                serde_json::from_slice(&raw).map_err(|error| error.to_string())?;
            let submission: super::ProductionUpgradeSubmission =
                serde_json::from_slice(&super::decode_hex(&receipt.submission_json_hex)?)
                    .map_err(|error| error.to_string())?;
            if receipt.checkpoint_evidence_sha256.as_ref() != Some(&prefix_digests[index])
                || submission.checkpoint_evidence_sha256.as_ref() != Some(&prefix_digests[index])
            {
                return Err(
                    "checkpoint suffix does not bind its frozen predecessor evidence".into(),
                );
            }
            let next_source = Source {
                revision: receipt.source_revision.clone(),
                tree_sha256: receipt.source_tree_sha256.clone(),
            };
            verify_source(&next_source, Some(&source.revision))?;
            source = next_source;
            module = receipt.after_module_sha256.clone();
            previous = Some(digest);
            Ok(receipt)
        });
    let terminal = if let Some(expected_module) = expected_module {
        super::validate_production_upgrade_receipts_from_start(
            &checkpoint.roots.gate_b_profile,
            &start,
            entries,
            &expected_module,
            super::CURRENT_STABLE_SCHEMA_VERSION,
            None,
        )?
    } else {
        drop(entries);
        start.terminal.clone()
    };
    Ok(VerifiedEvidence {
        checkpoint,
        terminal,
        module_sha256: module,
        source,
        evidence_sha256: hex(&Sha256::digest(bytes)),
    })
}

#[derive(Deserialize, Serialize, Clone)]
#[serde(deny_unknown_fields)]
pub(super) struct Source {
    pub revision: String,
    pub tree_sha256: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Activation {
    pub governance_operation_id: u64,
    pub finalized_block_number: u64,
    pub timelock_operation_id: String,
    pub transaction_hash: String,
    pub confirmed_generation: u8,
    pub confirmed_signed_at_ns: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Roots {
    pub gate_a_sha256: String,
    pub gate_b_sha256: String,
    pub seal_sha256: String,
    pub schedule_sha256: String,
    pub execute_sha256: String,
    pub activation: Activation,
    pub gate_b_created_at_unix: u64,
    pub deployment_block_number: u64,
    pub gate_b_profile: Profile,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Install {
    pub request_id: String,
    pub signed_install_sha256: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Checkpoint {
    pub schema_version: u8,
    pub kind: String,
    pub canister: String,
    pub network: String,
    pub controller: String,
    pub deployment_instance_id: String,
    pub module_sha256: String,
    pub runtime: LiveRuntimeBinding,
    pub lifecycle: ProductionLifecycleView,
    pub deposits_paused: bool,
    pub mint_authorization_epoch: u64,
    pub mint_authorization_ttl_seconds: u64,
    pub roots: Roots,
    pub source: Source,
    pub receipt_sha256: Vec<String>,
    pub installs: Vec<Install>,
    pub verified_at_unix: u64,
}

fn lowercase_digest(value: &str) -> bool {
    valid_sha256(value) && value.bytes().all(|byte| !byte.is_ascii_uppercase())
}

impl Checkpoint {
    pub(super) fn terminal(&self) -> super::ProductionUpgradeTerminal {
        super::ProductionUpgradeTerminal {
            runtime: self.runtime.clone(),
            lifecycle: self.lifecycle,
            deposits_paused: self.deposits_paused,
            observed_epoch: (
                self.mint_authorization_epoch,
                self.mint_authorization_ttl_seconds,
            ),
        }
    }
    fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1
            || self.kind != "production-upgrade-checkpoint"
            || self.network != "https://icp-api.io"
            || candid::Principal::from_text(&self.canister).is_err()
            || candid::Principal::from_text(&self.controller).is_err()
            || !self
                .deployment_instance_id
                .strip_prefix("0x")
                .is_some_and(lowercase_digest)
            || self.runtime.deployment_instance_id != self.deployment_instance_id
            || self.runtime.schema_version != super::CURRENT_STABLE_SCHEMA_VERSION
            || self.lifecycle != ProductionLifecycleView::Activated
            || self.mint_authorization_ttl_seconds == 0
            || self.verified_at_unix == 0
            || self.source.revision.len() != 40
            || !self
                .source
                .revision
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
            || self.receipt_sha256.is_empty()
            || self.installs.len() != self.receipt_sha256.len()
            || self
                .roots
                .activation
                .confirmed_signed_at_ns
                .parse::<u64>()
                .is_err()
        {
            return Err("invalid production checkpoint binding".into());
        }
        let profile = &self.roots.gate_b_profile;
        if profile.bridge_canister_id != self.canister
            || profile.deployment_instance_id != self.deployment_instance_id
            || profile.ic_host != self.network
            || profile.canister_schema_version != super::PREVIOUS_STABLE_SCHEMA_VERSION
            || profile.pause_principal != self.controller
            || self.roots.gate_b_created_at_unix > self.verified_at_unix
            || self.roots.deployment_block_number == 0
        {
            return Err("production checkpoint historical profile binding differs".into());
        }
        for digest in [
            &self.module_sha256,
            &self.source.tree_sha256,
            &self.roots.gate_a_sha256,
            &self.roots.gate_b_sha256,
            &self.roots.seal_sha256,
            &self.roots.schedule_sha256,
            &self.roots.execute_sha256,
        ]
        .into_iter()
        .chain(self.receipt_sha256.iter())
        {
            if !lowercase_digest(digest) {
                return Err("invalid production checkpoint digest".into());
            }
        }
        for digest in [
            &self.roots.activation.timelock_operation_id,
            &self.roots.activation.transaction_hash,
        ] {
            if !digest.strip_prefix("0x").is_some_and(lowercase_digest) {
                return Err("invalid production checkpoint activation binding".into());
            }
        }
        let mut receipts = BTreeSet::new();
        let mut requests = BTreeSet::new();
        let mut signatures = BTreeSet::new();
        for (receipt, install) in self.receipt_sha256.iter().zip(&self.installs) {
            if !receipts.insert(receipt)
                || !lowercase_digest(&install.request_id)
                || !lowercase_digest(&install.signed_install_sha256)
                || !requests.insert(&install.request_id)
                || !signatures.insert(&install.signed_install_sha256)
            {
                return Err("production checkpoint repeats or corrupts history".into());
            }
        }
        Ok(())
    }
}

// Every object is a deny_unknown_fields struct, with no Value/map escape hatch.
// Deserializing directly also rejects repeated keys instead of normalizing them.
pub(super) fn parse(bytes: &[u8]) -> Result<Checkpoint, String> {
    if bytes.len() as u64 > MAX_BYTES {
        return Err("production checkpoint exceeds 1 MiB".into());
    }
    let checkpoint: Checkpoint =
        serde_json::from_slice(bytes).map_err(|error| error.to_string())?;
    checkpoint.validate()?;
    Ok(checkpoint)
}

pub(super) fn read_candidate(path: &Path) -> Result<(Checkpoint, String), String> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(|error| error.to_string())?
        .take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    let checkpoint = parse(&bytes)?;
    Ok((checkpoint, hex(&Sha256::digest(&bytes))))
}

pub(super) fn read_approved(path: &Path) -> Result<(Checkpoint, String), String> {
    let (checkpoint, digest) = read_candidate(path)?;
    validate_approval(&checkpoint, &digest, APPROVED)?;
    Ok((checkpoint, digest))
}

fn validate_approval(
    checkpoint: &Checkpoint,
    digest: &str,
    registry: &[(&str, &str, &str)],
) -> Result<(), String> {
    let matches = registry
        .iter()
        .filter(|(canister, instance, _)| {
            *canister == checkpoint.canister && *instance == checkpoint.deployment_instance_id
        })
        .collect::<Vec<_>>();
    if matches.len() != 1 || matches[0].2 != digest {
        return Err("production checkpoint is not the active source-approved checkpoint".into());
    }
    Ok(())
}

#[cfg(test)]
pub(super) fn exercise_signed_suffix_fixture(profile: &Profile, raw: &[u8]) {
    tests::exercise_signed_suffix_fixture(profile, raw);
}

#[cfg(test)]
mod tests {
    use super::*;
    use candid::Encode;

    fn candidate() -> Vec<u8> {
        let mut profile = super::super::tests::valid_profile();
        profile.bridge_canister_id = "lb5i5-ziaaa-aaaar-qcgwq-cai".into();
        profile.deployment_instance_id = format!("0x{}", "a".repeat(64));
        profile.pause_principal = super::super::PRODUCTION_PAUSE_PRINCIPAL.into();
        profile.canister_schema_version = 35;
        serde_json::to_vec(&serde_json::json!({
            "schema_version": 1, "kind": "production-upgrade-checkpoint",
            "canister": "lb5i5-ziaaa-aaaar-qcgwq-cai", "network": "https://icp-api.io",
            "controller": super::super::PRODUCTION_PAUSE_PRINCIPAL,
            "deployment_instance_id": format!("0x{}", "a".repeat(64)), "module_sha256": "b".repeat(64),
            "runtime": {
                "base_chain_id": 8453, "bridge_contract": "c".repeat(40),
                "timelock_contract": "d".repeat(40), "deployment_instance_id": format!("0x{}", "a".repeat(64)),
                "minimum_withdrawal_id": "0", "ledger_canister_id": super::super::KINIC_LEDGER,
                "index_canister_id": super::super::KINIC_INDEX, "schema_version": 36,
                "expected_bridge_signer": "e".repeat(40),
                "evm_rpc_canister_id": super::super::OFFICIAL_EVM_RPC_CANISTER,
                "rpc_provider_urls_sha256": "1".repeat(64), "operational_config_sha256": "2".repeat(64)
            },
            "lifecycle": "Activated", "deposits_paused": false,
            "mint_authorization_epoch": 1, "mint_authorization_ttl_seconds": 60,
            "roots": {
                "gate_b_profile": profile, "gate_b_created_at_unix": 1, "deployment_block_number": 1,
                "gate_a_sha256": "3".repeat(64), "gate_b_sha256": "4".repeat(64),
                "seal_sha256": "5".repeat(64), "schedule_sha256": "6".repeat(64),
                "execute_sha256": "7".repeat(64),
                "activation": {"governance_operation_id": 1, "finalized_block_number": 2,
                    "timelock_operation_id": format!("0x{}", "8".repeat(64)),
                    "transaction_hash": format!("0x{}", "9".repeat(64)),
                    "confirmed_generation": 1, "confirmed_signed_at_ns": "100"}
            },
            "source": {"revision": "a".repeat(40), "tree_sha256": "b".repeat(64)},
            "receipt_sha256": ["c".repeat(64)],
            "installs": [{"request_id": "d".repeat(64), "signed_install_sha256": "e".repeat(64)}],
            "verified_at_unix": 100
        })).unwrap()
    }

    pub(super) fn exercise_signed_suffix_fixture(profile: &Profile, raw: &[u8]) {
        let mut receipt: super::super::ProductionCanisterUpgradeReceipt =
            serde_json::from_slice(raw).unwrap();
        let active = candid::Encode!(&super::super::ProductionLifecycleResultView::Ok(
            ProductionLifecycleView::Activated
        ))
        .unwrap();
        receipt.before_lifecycle = "Activated".into();
        receipt.after_lifecycle = "Activated".into();
        receipt.before_lifecycle_response_hex = hex(&active);
        receipt.after_lifecycle_response_hex = hex(&active);
        receipt.before_lifecycle_response_sha256 = hex(&Sha256::digest(&active));
        receipt.after_lifecycle_response_sha256 = hex(&Sha256::digest(&active));
        let (before, _, runtime, before_digest) = super::super::production_upgrade_query_state_any(
            &receipt.before_bridge_status_response_hex,
            &receipt.before_lifecycle_response_hex,
            &receipt.before_runtime_binding_response_hex,
            &receipt.before_storage_integrity_response_hex,
        )
        .unwrap();
        let (_, _, _, after_digest) = super::super::production_upgrade_query_state_any(
            &receipt.after_bridge_status_response_hex,
            &receipt.after_lifecycle_response_hex,
            &receipt.after_runtime_binding_response_hex,
            &receipt.after_storage_integrity_response_hex,
        )
        .unwrap();
        receipt.before_public_state_sha256 = before_digest;
        receipt.after_public_state_sha256 = after_digest;
        let mut checkpoint = parse(&candidate()).unwrap();
        checkpoint.canister = receipt.bridge_canister_id.clone();
        checkpoint.controller = receipt.executing_principal.clone();
        checkpoint.module_sha256 = receipt.before_module_sha256.clone();
        checkpoint.runtime = super::super::live_runtime_binding_from_view(&runtime);
        checkpoint.deployment_instance_id = checkpoint.runtime.deployment_instance_id.clone();
        checkpoint.deposits_paused = before.deposits_paused;
        checkpoint.mint_authorization_epoch = before.mint_authorization_epoch;
        checkpoint.mint_authorization_ttl_seconds = before.mint_authorization_ttl_seconds;
        checkpoint.verified_at_unix = receipt.executed_at_unix;
        checkpoint.roots.gate_b_profile = profile.clone();
        checkpoint.roots.gate_b_profile.canister_schema_version = 35;
        checkpoint.roots.gate_b_profile.pause_principal = checkpoint.controller.clone();
        checkpoint.source = Source {
            revision: receipt.source_revision.clone(),
            tree_sha256: receipt.source_tree_sha256.clone(),
        };
        let checkpoint_bytes = super::super::canonical_bytes(&checkpoint).unwrap();
        let receipt_bytes = serde_json::to_vec(&receipt).unwrap();
        let build = |checkpoint: &Checkpoint, raw: &[u8]| {
            let checkpoint_raw = super::super::canonical_bytes(checkpoint).unwrap();
            let mut evidence = Evidence {
                schema_version: 1,
                kind: "production-upgrade-checkpoint-evidence".into(),
                checkpoint_json_hex: hex(&checkpoint_raw),
                checkpoint_sha256: hex(&Sha256::digest(&checkpoint_raw)),
                entries: vec![],
            };
            let previous_evidence_digest = prefix_digest(&evidence, 0).unwrap();
            let mut receipt: super::super::ProductionCanisterUpgradeReceipt =
                serde_json::from_slice(raw).unwrap();
            let mut submission: super::super::ProductionUpgradeSubmission = serde_json::from_slice(
                &super::super::decode_hex(&receipt.submission_json_hex).unwrap(),
            )
            .unwrap();
            submission.checkpoint_evidence_sha256 = Some(previous_evidence_digest.clone());
            let submission_raw = serde_json::to_vec(&submission).unwrap();
            receipt.submission_json_hex = hex(&submission_raw);
            receipt.submission_json_sha256 = hex(&Sha256::digest(&submission_raw));
            receipt.checkpoint_evidence_sha256 = Some(previous_evidence_digest);
            let raw = serde_json::to_vec(&receipt).unwrap();
            evidence
                .entries
                .push(super::super::ProductionCanisterUpgradeChainEntry {
                    sequence: 0,
                    previous_receipt_sha256: checkpoint.receipt_sha256.last().cloned(),
                    receipt_sha256: hex(&Sha256::digest(&raw)),
                    receipt_json_hex: hex(&raw),
                });
            evidence
        };
        let evidence = build(&checkpoint, &receipt_bytes);
        let digest = hex(&Sha256::digest(&checkpoint_bytes));
        let registry = [(
            checkpoint.canister.as_str(),
            checkpoint.deployment_instance_id.as_str(),
            digest.as_str(),
        )];
        let bytes = super::super::canonical_bytes(&evidence).unwrap();
        let verified = verify_evidence(&bytes, &registry, |_, _| Ok(())).unwrap();
        assert_eq!(verified.module_sha256, receipt.after_module_sha256);
        assert_eq!(verified.terminal.runtime.schema_version, 36);
        assert!(
            verify_evidence(&bytes, &registry, |_, previous| if previous.is_some() {
                Err("nonancestor".into())
            } else {
                Ok(())
            })
            .is_err()
        );
        let mut replay_checkpoint = parse(&checkpoint_bytes).unwrap();
        replay_checkpoint.installs[0].request_id = receipt.request_id.clone();
        let replay = build(&replay_checkpoint, &receipt_bytes);
        let replay_registry = [(
            replay_checkpoint.canister.as_str(),
            replay_checkpoint.deployment_instance_id.as_str(),
            replay.checkpoint_sha256.as_str(),
        )];
        let error = verify_evidence(
            &super::super::canonical_bytes(&replay).unwrap(),
            &replay_registry,
            |_, _| Ok(()),
        )
        .err()
        .unwrap();
        assert!(error.contains("repeats an install request"), "{error}");
        for mutation in [
            "sequence",
            "previous",
            "module",
            "signature",
            "frozen",
            "time",
        ] {
            let mut evidence = build(&checkpoint, &receipt_bytes);
            let entry = &mut evidence.entries[0];
            if mutation == "sequence" {
                entry.sequence = 1;
            } else if mutation == "previous" {
                entry.previous_receipt_sha256 = None;
            } else {
                let mut receipt: super::super::ProductionCanisterUpgradeReceipt =
                    serde_json::from_slice(
                        &super::super::decode_hex(&entry.receipt_json_hex).unwrap(),
                    )
                    .unwrap();
                match mutation {
                    "module" => receipt.before_module_sha256 = "0".repeat(64),
                    "time" => receipt.executed_at_unix = checkpoint.verified_at_unix - 1,
                    "frozen" => receipt.checkpoint_evidence_sha256 = None,
                    _ => {
                        let mut submission: super::super::ProductionUpgradeSubmission =
                            serde_json::from_slice(
                                &super::super::decode_hex(&receipt.submission_json_hex).unwrap(),
                            )
                            .unwrap();
                        submission.signed_update_hex = "00".into();
                        submission.signed_update_sha256 = hex(&Sha256::digest([0]));
                        let raw = serde_json::to_vec(&submission).unwrap();
                        receipt.submission_json_hex = hex(&raw);
                        receipt.submission_json_sha256 = hex(&Sha256::digest(&raw));
                    }
                }
                let raw = serde_json::to_vec(&receipt).unwrap();
                entry.receipt_json_hex = hex(&raw);
                entry.receipt_sha256 = hex(&Sha256::digest(&raw));
            }
            assert!(
                verify_evidence(
                    &super::super::canonical_bytes(&evidence).unwrap(),
                    &registry,
                    |_, _| Ok(())
                )
                .is_err(),
                "{mutation}"
            );
        }
    }

    #[test]
    fn checkpoint_requires_exact_active_hash_and_instance() {
        let bytes = candidate();
        let checkpoint = parse(&bytes).unwrap();
        let digest = hex(&Sha256::digest(&bytes));
        let registry = [(
            checkpoint.canister.as_str(),
            checkpoint.deployment_instance_id.as_str(),
            digest.as_str(),
        )];
        assert!(validate_approval(&checkpoint, &digest, &registry).is_ok());
        assert!(validate_approval(&checkpoint, &digest, &[]).is_err());
        assert!(validate_approval(&checkpoint, &"0".repeat(64), &registry).is_err());
        assert!(validate_approval(&checkpoint, &digest, &[registry[0], registry[0]]).is_err());
        let mut other = parse(&bytes).unwrap();
        other.deployment_instance_id = "0".repeat(64);
        assert!(validate_approval(&other, &digest, &registry).is_err());
    }

    #[test]
    fn checkpoint_empty_suffix_needs_only_the_approved_terminal() {
        let bytes = candidate();
        let checkpoint = parse(&bytes).unwrap();
        let digest = hex(&Sha256::digest(&bytes));
        let registry = [(
            checkpoint.canister.as_str(),
            checkpoint.deployment_instance_id.as_str(),
            digest.as_str(),
        )];
        let evidence = Evidence {
            schema_version: 1,
            kind: "production-upgrade-checkpoint-evidence".into(),
            checkpoint_json_hex: hex(&bytes),
            checkpoint_sha256: digest.clone(),
            entries: vec![],
        };
        let bytes = super::super::canonical_bytes(&evidence).unwrap();
        let calls = std::cell::Cell::new(0);
        let verified = verify_evidence(&bytes, &registry, |source, predecessor| {
            calls.set(calls.get() + 1);
            assert!(predecessor.is_none());
            assert_eq!(source.revision, checkpoint.source.revision);
            Ok(())
        })
        .unwrap();
        assert_eq!(calls.get(), 1);
        assert_eq!(verified.module_sha256, checkpoint.module_sha256);
        assert_eq!(verified.terminal.runtime.schema_version, 36);
        assert!(verify_evidence(&bytes, &[], |_, _| panic!(
            "unapproved checkpoint must fail before source access"
        ))
        .is_err());
        assert!(verify_evidence(&bytes, &registry, |_, _| Err(
            "missing terminal Git object".into()
        ))
        .is_err());
        let mut tampered = evidence;
        tampered.checkpoint_sha256 = "0".repeat(64);
        assert!(verify_evidence(
            &super::super::canonical_bytes(&tampered).unwrap(),
            &registry,
            |_, _| panic!("tampering must fail before source access")
        )
        .is_err());
    }

    #[test]
    fn checkpoint_rejects_unsupported_schema_runtime_drift_and_replay() {
        for field in ["schema_version", "runtime", "installs", "receipt_sha256"] {
            let mut value: serde_json::Value = serde_json::from_slice(&candidate()).unwrap();
            match field {
                "schema_version" => value[field] = 2.into(),
                "runtime" => value[field]["deployment_instance_id"] = "0".repeat(64).into(),
                "installs" => {
                    let install = value[field][0].clone();
                    value[field].as_array_mut().unwrap().push(install);
                    value["receipt_sha256"]
                        .as_array_mut()
                        .unwrap()
                        .push("f".repeat(64).into());
                }
                _ => value[field][0] = "invalid".into(),
            }
            assert!(
                parse(&serde_json::to_vec(&value).unwrap()).is_err(),
                "{field}"
            );
        }
        let bytes = String::from_utf8(candidate()).unwrap();
        let duplicated = bytes.replace(
            "\"base_chain_id\":8453",
            "\"base_chain_id\":8453,\"base_chain_id\":8453",
        );
        assert!(parse(duplicated.as_bytes())
            .err()
            .unwrap()
            .contains("duplicate field"));
    }

    #[test]
    fn checkpoint_rejects_oversize_before_decoding() {
        assert!(parse(&vec![b' '; MAX_BYTES as usize + 1])
            .err()
            .unwrap()
            .contains("1 MiB"));
    }

    #[test]
    fn checkpoint_rejects_duplicate_and_unknown_fields() {
        for bytes in [
            br#"{"schema_version":1,"schema_version":1}"#.as_slice(),
            br#"{"unrecognized":1}"#.as_slice(),
        ] {
            let error = parse(bytes).err().unwrap();
            assert!(error.contains("duplicate field") || error.contains("unknown field"));
        }
    }

    #[test]
    fn checkpoint_registry_has_one_active_hash_per_instance() {
        let mut instances = BTreeSet::new();
        for (canister, instance, digest) in APPROVED {
            assert!(instances.insert((canister, instance)));
            assert!(candid::Principal::from_text(canister).is_ok());
            assert!(instance.strip_prefix("0x").is_some_and(lowercase_digest));
            assert!(lowercase_digest(digest));
        }
    }
}
