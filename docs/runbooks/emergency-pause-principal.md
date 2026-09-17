# Emergency pause principal operations

The permanent human administration credential is one IC hardware identity authorized only for Bridge Canister safety actions. Use no release approver, IC finance principal, or human EVM administration key. During initial activation only, a production-only upgrade migrates the deployed SNS Root pause principal to the production controller identity, which temporarily holds both controller and emergency-pause roles. Operators separately decide when to end this exception; there is no automatic expiry or rotation.

1. When creating a steady-state emergency pause hardware identity, ensure its principal differs from SNS Governance, Fee Recipient, and controller. Initial-activation production-identity overlap is the explicit exception above.
2. From the same identity, run `emergency_pause` and Governance relayer `drain-emergency` on a test canister, rehearsing IC pause, both Base flow pauses, and cancellation of recorded pending Timelock operations.
3. Record principal, actual request ID, audit sequence/digest, and ordered incident/detection/acknowledgement/both-side-pause timestamps in schema v4 `monitor-drill.json`. 5/15/60 are post-publication monitoring targets; post-unpause Gate C evidence requires successful pause/cancel paths. This evidence does not authorize Gate B, activation, or controller handover. Store no secrets, seeds, or device backups.
4. Future rotation runs only through the fixed KINIC SNS Governance generic function after operators explicitly approve timing. The former principal loses authority once rotation completes.

The Bridge Canister derives Mint Signer, Governance Operator, Runtime Administrator, and Independent Canceller per role. Mint Signer only signs EIP-712 and needs no ETH. Permit ETH replenishment only to Base-sending control-plane roles; grant no contract roles to funding senders.
The pause principal may only prepare signatures for Base pause/recorded Timelock cancellation, retrieve signed artifacts, request explicit replacements, and notify Finalized confirmation. It cannot change Service Fees, schedule/execute activation, or resume.
