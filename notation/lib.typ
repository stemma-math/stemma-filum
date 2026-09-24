// How a Stemma line looks on the page.
//
// The arguments are not decoration. `id`, `uses`, `central`, `of`,
// `bridges` and `author` are read out of the source by Stemma itself —
// that is how the apparatus and the descent are built — so they live in
// the document where their author can see them, and not in a database.
// This file decides only what they look like on paper.

#let counted = counter("stemma-statement")

// The siglum, small and out of the way. It is what a reference points
// at, so it has to be readable, and it is not part of the mathematics,
// so it does not compete with it.
#let siglum(id) = if id == none [] else {
  h(1fr)
  text(size: 0.85em, fill: luma(45%), raw(id))
}

// One numbered statement. `slanted` is the old convention: what is
// asserted is set in italics, what is introduced or noted is not.
#let statement(kind, slanted, id, name, central, body) = {
  counted.step()

  block(
    width: 100%,
    above: 1.2em,
    below: 1.2em,
    inset: (left: if central { 0.9em } else { 0pt }),
    stroke: if central { (left: 1.5pt + luma(25%)) } else { none },
    {
      context {
        let number = counted.get().first()

        strong[#kind #number.]
        if name != none [ #strong[(#name)]]
        siglum(id)
      }
      parbreak()
      if slanted { emph(body) } else { body }
    },
  )
}

// The question the line exists to answer. It is not mathematics and it
// declares nothing: it is what everything below is for, so it is set
// apart and read first.
#let question(body) = block(
  width: 100%,
  fill: luma(96%),
  inset: 0.9em,
  radius: 2pt,
  {
    text(size: 0.8em, fill: luma(40%), tracking: 0.08em, upper[The question])
    parbreak()
    body
  },
)

#let definition(id: none, name: none, central: false, uses: (), bridges: (), author: none, body) = statement("Definition", false, id, name, central, body)

#let theorem(id: none, name: none, central: false, uses: (), bridges: (), author: none, body) = statement("Theorem", true, id, name, central, body)

#let lemma(id: none, name: none, central: false, uses: (), bridges: (), author: none, body) = statement("Lemma", true, id, name, central, body)

#let proposition(id: none, name: none, central: false, uses: (), bridges: (), author: none, body) = statement("Proposition", true, id, name, central, body)

#let corollary(id: none, name: none, central: false, uses: (), bridges: (), author: none, body) = statement("Corollary", true, id, name, central, body)

#let remark(id: none, name: none, central: false, uses: (), bridges: (), author: none, body) = statement("Remark", false, id, name, central, body)

// A proof is written for a statement and is not one itself, so it is
// not numbered and carries no siglum of its own.
#let proof(of: none, author: none, body) = block(width: 100%, above: 1.2em, below: 1.2em, {
  emph[Proof#if of != none [ of #raw(of)].]
  [ ]
  body
  h(1fr)
  sym.square.stroked
})
