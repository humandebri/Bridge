import { sha256, type Hex } from "viem"

export function runtimeBytecodeSha256(bytecode: Hex): Hex {
  return sha256(bytecode)
}
