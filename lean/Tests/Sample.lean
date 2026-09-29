import Stemma

namespace Sample

def predicate (value : Nat) : Prop := value = value

@[stemma "sample/thm:base"]
theorem base : predicate 0 := rfl

theorem helper : predicate 0 := base

@[stemma "sample/thm:main"]
theorem main : predicate 0 := helper

-- Deliberately unfinished: the extractor must report the `sorry`.
set_option warn.sorry false in
@[stemma "sample/thm:unfinished"]
theorem unfinished : True := by sorry

@[stemma "sample/thm:many"]
theorem first : True := True.intro

@[stemma "sample/thm:many"]
theorem second : True := True.intro

@[stemma "sample/thm:missing"]
theorem missing : True := True.intro

end Sample
