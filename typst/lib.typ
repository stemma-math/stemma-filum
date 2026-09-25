// How a Stemma line looks on the page.
//
// A line is kept as a list of blocks and typeset as one call per block,
// written by Stemma: prose, definitions, theorems and proofs. Every call
// also places a `metadata` element carrying the block's fields, in the
// order the blocks come, so what a line declares can be read back out of
// the document with `query`. This file decides only what they look like.

// Definitions and theorems are numbered together, the way a reader
// counts them, and are figures so that `@siglum` refers to them.
#let statement-kind = "stemma-statement"

// What every block leaves in the document besides what it shows.
#let data(kind, ..fields) = metadata((stemma: "block", kind: kind, ..fields.named()))

// The whole line: its title, what it is about, and the rules that make
// statements look like statements rather than figures.
#let stemma(title: none, description: none, body) = {
  set document(title: title)
  show figure.where(kind: statement-kind): set block(breakable: true)
  show figure.where(kind: statement-kind): set align(start)
  show figure.where(kind: statement-kind): it => it.body

  metadata((stemma: "line", title: title, description: description))
  if title != none {
    block(below: 0.6em, text(size: 1.6em, weight: "bold", title))
  }
  if description != none {
    block(below: 1.6em, text(fill: luma(35%), description))
  }
  body
}

// The siglum, small and out of the way. It is what a reference points
// at, so it has to be readable, and it is not part of the mathematics,
// so it does not compete with it.
#let siglum-mark(siglum) = {
  h(1fr)
  text(size: 0.85em, fill: luma(45%), raw(siglum))
}

// Prose between the mathematics. It is not numbered and declares nothing.
#let prose(id: none, body) = {
  data("prose", id: id)
  body
}

// One numbered statement. `slanted` is the old convention: what is
// asserted is set in italics, what is introduced is not.
#let statement(kind, slanted, id, environment, siglum, title, central, proof, uses, body) = figure(
  kind: statement-kind,
  supplement: environment,
  numbering: "1",
  outlined: false,
  {
    data(
      kind,
      id: id,
      environment: environment,
      siglum: siglum,
      title: title,
      central: central,
      proof: proof,
      uses: uses,
    )
    block(
      width: 100%,
      above: 1.2em,
      below: 1.2em,
      inset: (left: if central { 0.9em } else { 0pt }),
      stroke: if central { (left: 1.5pt + luma(25%)) } else { none },
      {
        context {
          let number = counter(figure.where(kind: statement-kind)).get().first()
          strong[#environment #number.]
          if title != none [ #strong[(#title)]]
          if siglum != none { siglum-mark(siglum) }
        }
        parbreak()
        if slanted { emph(body) } else { body }
      },
    )
  },
)

#let definition(
  id: none,
  environment: "Definition",
  siglum: none,
  title: none,
  central: true,
  proof: false,
  uses: (),
  body,
) = statement("definition", false, id, environment, siglum, title, central, proof, uses, body)

#let theorem(
  id: none,
  environment: "Theorem",
  siglum: none,
  title: none,
  central: false,
  proof: true,
  uses: (),
  body,
) = statement("theorem", true, id, environment, siglum, title, central, proof, uses, body)

// A proof is written for a statement and is not one itself, so it is
// not numbered. Straight after its statement it only says it is a
// proof; anywhere else it says of what. An empty one says it is still
// to be written, since a statement carrying a proof always has one.
#let proof(id: none, of: none, target: none, adjacent: false, body) = {
  data("proof", id: id, of: of)
  block(width: 100%, above: 1.2em, below: 1.2em, {
    if adjacent or target == none {
      emph[Proof.]
    } else {
      emph[Proof of #ref(label(target)).]
    }
    [ ]
    if body == [] {
      text(fill: luma(50%))[Not written yet.]
    } else {
      body
    }
    h(1fr)
    sym.square.stroked
  })
}
