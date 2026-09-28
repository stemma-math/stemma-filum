import Lean
import Stemma.Attribute
import Stemma.Sha256

namespace Stemma

open Lean

/-- A declaration attached to a Stemma block. -/
structure MarkedDeclaration where
  name : String
  module : String
  statement : String
  sorryFree : Bool
  axioms : Array String
  reaches : Array String
  statementClosure : String
  deriving Repr, ToJson

/-- Every declaration attached to one block. -/
structure Mark where
  block : BlockId
  declarations : Array MarkedDeclaration
  deriving Repr, ToJson

/-- An informational report for a block linked by more than one declaration. -/
structure DuplicateMark where
  block : BlockId
  declarations : Array String
  deriving Repr, ToJson

/-- The machine-readable result emitted by `stemma-extract`. -/
structure Report where
  version : Nat := 1
  marks : Array Mark
  brokenMarks : Array BlockId
  duplicateMarks : Array DuplicateMark
  deriving Repr, ToJson

private def markedDeclarations (env : Environment) : Array (BlockId × Name) := Id.run do
  let mut declarations := #[]
  for index in [:env.header.moduleNames.size] do
    for (name, block) in markAttribute.ext.getModuleEntries env index do
      declarations := declarations.push (block, name)
  let (localNames, localMarks) := markAttribute.ext.getState env
  for name in localNames do
    if let some block := localMarks.find? name then
      declarations := declarations.push (block, name)
  return declarations.qsort fun left right =>
    left.1 < right.1 || (left.1 == right.1 && Name.lt left.2 right.2)

private def moduleFor (env : Environment) (name : Name) : String :=
  match env.getModuleIdxFor? name with
  | some index => env.header.moduleNames[index]!.toString
  | none => env.mainModule.toString

private def declarationExpressions (info : ConstantInfo) (includeProofs : Bool) : Array Expr :=
  match info with
  | .axiomInfo value => #[value.type]
  | .defnInfo value => #[value.type, value.value]
  | .thmInfo value => if includeProofs then #[value.type, value.value] else #[value.type]
  | .opaqueInfo value => #[value.type, value.value]
  | .quotInfo value => #[value.type]
  | .inductInfo value => #[value.type]
  | .ctorInfo value => #[value.type]
  | .recInfo value => #[value.type]

private def directDependencies (info : ConstantInfo) (includeProofs : Bool) : NameSet :=
  (declarationExpressions info includeProofs).foldl
    (fun found expression =>
      expression.getUsedConstants.foldl (fun found name => found.insert name) found)
    {}

private partial def reachableMarked
    (env : Environment) (marked : NameSet) (root : Name) : Array Name := Id.run do
  let some rootInfo := env.find? root | return #[]
  let mut pending := (directDependencies rootInfo true).toArray
  let mut seen : NameSet := {root}
  let mut reached : NameSet := {}
  while let some current := pending.back? do
    pending := pending.pop
    if seen.contains current then
      continue
    seen := seen.insert current
    if marked.contains current then
      reached := reached.insert current
      continue
    if let some info := env.find? current then
      pending := pending ++ (directDependencies info true).toArray
  return reached.toArray.qsort Name.lt

private partial def statementClosureEntries (env : Environment) (root : Name) : Array String := Id.run do
  let some rootInfo := env.find? root | return #[]
  let mut pending := rootInfo.type.getUsedConstants
  let mut seen : NameSet := {}
  let mut entries := #[s!"statement\n{repr rootInfo.type}"]
  while let some current := pending.back? do
    pending := pending.pop
    if seen.contains current then
      continue
    seen := seen.insert current
    if let some info := env.find? current then
      let expressions := declarationExpressions info false
      let kind := match info with
        | .axiomInfo _ => "axiom"
        | .defnInfo _ => "definition"
        | .thmInfo _ => "theorem"
        | .opaqueInfo _ => "opaque"
        | .quotInfo _ => "quotient"
        | .inductInfo _ => "inductive"
        | .ctorInfo _ => "constructor"
        | .recInfo _ => "recursor"
      entries := entries.push s!"{current}\n{kind}\n{repr expressions}"
      for expression in expressions do
        pending := pending ++ expression.getUsedConstants
      if let .inductInfo value := info then
        pending := pending ++ value.ctors.toArray
  return entries.qsort (· < ·)

private def fingerprint (entries : Array String) : String :=
  "sha256:" ++ sha256 (String.intercalate "\n--\n" entries.toList)

private def printedStatement (env : Environment) (info : ConstantInfo) : IO String :=
  Lean.PrettyPrinter.ppExprLegacy env {} {} {} info.type |>.map toString

private def usedAxioms (env : Environment) (name : Name) : IO (Array Name) :=
  Lean.Core.CoreM.toIO' (Lean.collectAxioms name)
    { fileName := "<stemma-extract>", fileMap := default }
    { env }

private def extractDeclaration
    (env : Environment) (marked : NameSet) (name : Name) : IO MarkedDeclaration := do
  let some info := env.find? name
    | throw <| IO.userError s!"marked declaration `{name}` is missing from the environment"
  let axioms ← usedAxioms env name
  let reaches := reachableMarked env marked name
  return {
    name := name.toString
    module := moduleFor env name
    statement := ← printedStatement env info
    sorryFree := !axioms.contains ``sorryAx
    axioms := axioms.map toString
    reaches := reaches.map toString
    statementClosure := fingerprint (statementClosureEntries env name)
  }

/-- Extracts every Stemma mark present in `env`. -/
def extract (env : Environment) (knownBlocks : Array BlockId) : IO Report := do
  let declarations := markedDeclarations env
  let marked := declarations.foldl (fun found (_, name) => found.insert name) {}
  let known := knownBlocks.foldl (fun found block => found.insert block) ({} : Std.HashSet String)
  let mut grouped : Std.HashMap String (Array Name) := {}
  for (block, name) in declarations do
    grouped := grouped.alter block fun names => some <| names.getD #[] |>.push name
  let blocks := grouped.toArray.map (·.1) |>.qsort (· < ·)
  let mut marks := #[]
  let mut brokenMarks := #[]
  let mut duplicateMarks := #[]
  for block in blocks do
    let names := grouped[block]!.qsort Name.lt
    let entries ← names.mapM (extractDeclaration env marked)
    marks := marks.push { block, declarations := entries }
    unless known.contains block do
      brokenMarks := brokenMarks.push block
    if names.size > 1 then
      duplicateMarks := duplicateMarks.push {
        block
        declarations := names.map toString
      }
  return { marks, brokenMarks, duplicateMarks }

end Stemma
