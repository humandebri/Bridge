import Lake

open Lake DSL

package bridgeSpec

lean_lib BridgeSpec

@[default_target]
lean_exe bridge_spec_vectors where
  root := `Main
