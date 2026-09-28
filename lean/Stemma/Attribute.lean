import Lean

namespace Stemma

/-- A stable identifier for a block in a Stemma line. -/
abbrev BlockId := String

/--
Links a Lean declaration to a Stemma block.

The value is written as `<line>/<block>`, for example
`@[stemma "compactness/thm:main"]`.
-/
syntax (name := stemmaAttr) "stemma" ppSpace str : attr

initialize markAttribute : Lean.ParametricAttribute BlockId ←
  Lean.registerParametricAttribute {
    name := `stemmaAttr
    descr := "link a declaration to a Stemma block"
    getParam := fun _ stx => do
      let `(attr| stemma $block:str) := stx
        | Lean.throwError "invalid `[stemma]` attribute syntax"
      let block := block.getString
      if block.trimAscii.isEmpty then
        Lean.throwError "a Stemma block identifier cannot be empty"
      return block
  }

/-- The block linked to `declaration`, if it has a `stemma` attribute. -/
def markFor? (env : Lean.Environment) (declaration : Lean.Name) : Option BlockId :=
  markAttribute.getParam? env declaration

end Stemma
