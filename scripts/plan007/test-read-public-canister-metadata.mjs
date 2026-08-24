import assert from "node:assert/strict"
import test from "node:test"

import { LookupPathStatus } from "@icp-sdk/core/agent"

import {
  classifyMetadataLookup,
  publicMetadataPath,
  readPublicCanisterMetadata,
} from "./read-public-canister-metadata.mjs"

test("classifies only a certified absent path as missing metadata", () => {
  assert.deepEqual(
    classifyMetadataLookup({ status: LookupPathStatus.Absent }),
    { status: "absent" },
  )
  assert.deepEqual(
    classifyMetadataLookup({
      status: LookupPathStatus.Found,
      value: new TextEncoder().encode("service : {}"),
    }),
    { status: "present", value: "service : {}" },
  )
  assert.throws(
    () => classifyMetadataLookup({ status: LookupPathStatus.Unknown }),
    /unknown path/,
  )
  assert.throws(
    () => classifyMetadataLookup({ status: LookupPathStatus.Error }),
    /invalid path/,
  )
})

test("rejects malformed lookup results and invalid metadata UTF-8", () => {
  assert.throws(() => classifyMetadataLookup({ status: "future-status" }), /unsupported status/)
  assert.throws(
    () => classifyMetadataLookup({ status: LookupPathStatus.Found, value: Uint8Array.of(0xff) }),
    /encoded data was not valid|valid for encoding/i,
  )
})

test("verifies the read-state certificate before classifying metadata", async () => {
  const rootKey = Uint8Array.of(1, 2, 3)
  const certificateBytes = Uint8Array.of(4, 5, 6)
  let requestedPath
  const agent = {
    rootKey,
    async readState(canisterId, request) {
      assert.equal(canisterId.toText(), "aaaaa-aa")
      requestedPath = request.paths[0]
      return { certificate: certificateBytes }
    },
  }
  const createCertificate = async options => {
    assert.equal(options.agent, agent)
    assert.equal(options.principal.canisterId.toText(), "aaaaa-aa")
    assert.equal(options.certificate, certificateBytes)
    assert.equal(options.rootKey, rootKey)
    return {
      lookup_path(path) {
        assert.equal(path, requestedPath)
        return { status: LookupPathStatus.Absent }
      },
    }
  }

  assert.deepEqual(
    await readPublicCanisterMetadata(
      "https://icp-api.io",
      "aaaaa-aa",
      "candid:service",
      { agent, createCertificate },
    ),
    { status: "absent" },
  )
  assert.deepEqual(
    requestedPath,
    publicMetadataPath({ toUint8Array: () => new Uint8Array() }, "candid:service"),
  )
})

test("propagates transport and certificate verification failures", async () => {
  const transportFailure = new Error("request timed out")
  await assert.rejects(
    readPublicCanisterMetadata("https://icp-api.io", "aaaaa-aa", "candid:service", {
      agent: {
        rootKey: Uint8Array.of(1),
        async readState() {
          throw transportFailure
        },
      },
    }),
    error => error === transportFailure,
  )

  const certificateFailure = new Error("certificate verification failed")
  await assert.rejects(
    readPublicCanisterMetadata("https://icp-api.io", "aaaaa-aa", "candid:service", {
      agent: {
        rootKey: Uint8Array.of(1),
        async readState() {
          return { certificate: Uint8Array.of(2) }
        },
      },
      async createCertificate() {
        throw certificateFailure
      },
    }),
    error => error === certificateFailure,
  )
})
