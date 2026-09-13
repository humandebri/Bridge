import Lean

namespace BridgeSpec.AuditExport

/-- Print elaborated types and definition bodies, never theorem proof terms.
This is review output, not an independent kernel or a semantic equivalence checker. -/
def declarationJson (name : Lean.Name) : Lean.MetaM Lean.Json := do
  let info ← Lean.getConstInfo name
  let type ← Lean.Meta.ppExpr info.type
  let body ← match info with
    | .defnInfo value => pure (Lean.Json.str (← Lean.Meta.ppExpr value.value).pretty)
    | _ => pure Lean.Json.null
  pure <| Lean.Json.mkObj [
    ("name", Lean.toJson name.toString),
    ("type", Lean.toJson type.pretty),
    ("definition", body)]

end BridgeSpec.AuditExport
