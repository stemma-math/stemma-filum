import Lean
import Stemma.Extractor

open Lean

private structure Arguments where
  blocks : System.FilePath
  modules : Array Name

private def usage : String :=
  "usage: stemma-extract --blocks BLOCKS.json MODULE [MODULE ...]"

private def parseArguments (arguments : List String) : Except String Arguments := do
  let rec loop (remaining : List String) (blocks : Option System.FilePath)
      (modules : Array Name) : Except String Arguments :=
    match remaining with
    | [] =>
      match blocks with
      | none => .error "missing `--blocks BLOCKS.json`"
      | some blocks =>
        if modules.isEmpty then
          .error "at least one module is required"
        else
          .ok { blocks, modules }
    | ["--blocks"] => .error "`--blocks` needs a path"
    | "--blocks" :: path :: rest => loop rest (some path) modules
    | argument :: rest =>
      if argument.startsWith "-" then
        .error s!"unknown option `{argument}`"
      else
        loop rest blocks (modules.push argument.toName)
  loop arguments none #[]

private def readBlocks (path : System.FilePath) : IO (Array Stemma.BlockId) := do
  let source ← IO.FS.readFile path
  let json ← IO.ofExcept <| Json.parse source
  IO.ofExcept <| fromJson? json

private def loadEnvironment (modules : Array Name) : IO Environment := do
  Lean.initSearchPath (← Lean.findSysroot)
  unsafe Lean.enableInitializersExecution
  let imports := modules.map fun module => { module : Import }
  Lean.importModules (loadExts := true) imports {}

def main (arguments : List String) : IO UInt32 := do
  let parsed ← match parseArguments arguments with
    | .ok parsed => pure parsed
    | .error message =>
        IO.eprintln s!"{message}\n{usage}"
        return 2
  try
    let blocks ← readBlocks parsed.blocks
    let env ← loadEnvironment parsed.modules
    let report ← Stemma.extract env blocks
    IO.println (toJson report).pretty
    return 0
  catch error =>
    IO.eprintln error
    return 1
