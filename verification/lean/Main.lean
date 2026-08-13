import BridgeSpec.Vectors
import BridgeSpec.Protocol
import BridgeSpec.ClaimContracts
import BridgeSpec.ControlPlane
import BridgeSpec.GlobalHistory
import BridgeSpec.Liveness

def main : IO Unit :=
  IO.print BridgeSpec.Vectors.document
