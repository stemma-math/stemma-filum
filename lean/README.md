# Stemma Lean package

This package links Lean declarations to Stemma blocks and extracts the formal
information that Stemma audits.

Import `Stemma`, then mark declarations with a stable line and block identifier:

```lean
import Stemma

@[stemma "compactness/thm:main"]
theorem finite_subcover : True := by
  trivial
```

Build a library before extracting it. The extractor accepts the JSON array of
block identifiers from Stemma and one or more compiled root modules:

```sh
lake build
lake exe stemma-extract --blocks blocks.json MyGroup
```

It writes JSON containing the declarations attached to each mark, their modules
and Lean-printed statements, axiom and `sorry` status, the other marked
declarations they reach through unmarked declarations, and a fingerprint of
each statement closure. `brokenMarks` names marks absent from the supplied block
list. `duplicateMarks` is informational: several declarations may deliberately
back one block.

Run the package tests with:

```sh
lake test
```
