import Lean
import Stemma.Extractor
import Tests.Sample

open Lean

private def loadSample : IO Environment := do
  Lean.initSearchPath (← Lean.findSysroot)
  unsafe Lean.enableInitializersExecution
  Lean.importModules (loadExts := true) #[{ module := `Tests.Sample }] {}

private def fail (message : String) : IO α :=
  throw <| IO.userError message

private def mark! (report : Stemma.Report) (block : String) : IO Stemma.Mark :=
  report.marks.find? (·.block == block) |>.getDM (fail s!"missing mark `{block}`")

private def declaration! (mark : Stemma.Mark) : IO Stemma.MarkedDeclaration :=
  mark.declarations[0]?.getDM (fail s!"mark `{mark.block}` has no declarations")

def main : IO UInt32 := do
  unless Stemma.sha256 "" == "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855" do
    fail "SHA-256 does not match the empty-input test vector"
  unless Stemma.sha256 "abc" == "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad" do
    fail "SHA-256 does not match the abc test vector"
  let env ← loadSample
  let report ← Stemma.extract env #[
    "sample/thm:base",
    "sample/thm:main",
    "sample/thm:unfinished",
    "sample/thm:many"
  ]
  let main ← mark! report "sample/thm:main"
  let mainDeclaration ← declaration! main
  let baseDeclaration ← declaration! (← mark! report "sample/thm:base")
  unless mainDeclaration.reaches == #["Sample.base"] do
    fail s!"main reaches {mainDeclaration.reaches}, not Sample.base"
  unless mainDeclaration.sorryFree do
    fail "the completed theorem was reported as containing sorry"
  unless mainDeclaration.module == "Tests.Sample" do
    fail s!"unexpected module: {mainDeclaration.module}"
  unless mainDeclaration.statement == "Sample.predicate 0" do
    fail s!"unexpected printed statement: {mainDeclaration.statement}"
  unless mainDeclaration.statementClosure == baseDeclaration.statementClosure do
    fail "proof-only differences changed the statement closure"
  let unfinished ← mark! report "sample/thm:unfinished"
  let unfinishedDeclaration ← declaration! unfinished
  if unfinishedDeclaration.sorryFree then
    fail "the unfinished theorem was reported as sorry-free"
  unless unfinishedDeclaration.axioms.contains "sorryAx" do
    fail s!"the unfinished theorem's axioms omit sorryAx: {unfinishedDeclaration.axioms}"
  unless report.brokenMarks == #["sample/thm:missing"] do
    fail s!"unexpected broken marks: {report.brokenMarks}"
  unless report.duplicateMarks.map (·.block) == #["sample/thm:many"] do
    fail s!"unexpected duplicate marks: {report.duplicateMarks.map (·.block)}"
  unless mainDeclaration.statementClosure.startsWith "sha256:" do
    fail s!"unexpected statement fingerprint: {mainDeclaration.statementClosure}"
  IO.println "Stemma Lean extractor tests passed"
  return 0
