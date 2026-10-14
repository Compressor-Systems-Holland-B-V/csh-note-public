# SKOS glossaries for znote

znote hides company names, people, customers, projects and other terms from the assistant by
replacing them with tokens such as `⟦CUSTOMER_3⟧`. Which terms to hide comes from a glossary in
[SKOS](https://www.w3.org/TR/skos-reference/), imported in Settings > Privacy. This page describes
the part of SKOS znote reads, for whoever builds a tool that writes these files.

## Files

- Turtle (`.ttl`) or RDF/XML (`.rdf`, `.xml`, `.owl`).
- At most 20 MB and 200 000 statements. Every label is at most 200 characters and on one line.
- Anything else is refused with the reason, and for a syntax error the line it failed on.
- A new import replaces the previous one. Choices made in znote ("Don't hide this", "Always hide
  this") are kept apart and survive a new import.

## What znote reads

| SKOS | Used as |
|---|---|
| `skos:Concept` | A term. Anything with a `skos:prefLabel` that is in the hierarchy counts as well. |
| `skos:prefLabel` | The main spelling. English is preferred when there are several languages. |
| `skos:altLabel` | Another spelling, matched and hidden too. |
| `skos:hiddenLabel` | A spelling that is matched but never shown (typos, internal codes). |
| `skos:topConceptOf`, `skos:hasTopConcept` | Marks a category. |
| `skos:broader`, `skos:narrower` | The hierarchy. Also used by the knowledge base to find related notes. |
| `skos:related` | A link between two terms, used by the knowledge base. |
| `skos:definition`, `skos:scopeNote` | A note about the term. It is hidden like any other text before the assistant sees it. |
| `znote:mask false` | Do not hide this term. The prefix is `@prefix znote: <urn:znote:skos#>`. |

Everything else is ignored.

## Categories and kinds

- A **top concept** is a category, such as Customer or Person. Categories are never hidden.
- A concept with no `skos:broader` that has narrower concepts is treated as a category too.
- Every other concept is hidden, unless it has `znote:mask false`.
- The **kind** in a token is the `prefLabel` of the category above the term, found by following
  `skos:broader` up: "Customer" becomes `CUSTOMER`. Kinds are plain upper-case ASCII of at most 16
  characters; anything else in the label is dropped. A term with no category gets the kind `TERM`.

## Matching

- Every label of every hidden term is matched in the text, ignoring case, at word boundaries.
  The longest match wins.
- A label is never matched across a line break, or when it is split by Typst markup such as
  `*Acme* Corp`.
- All spellings of one term share its number: the `prefLabel` "Acme Corporation" is
  `⟦CUSTOMER_3⟧`, and other spellings get `~1`, `~2`, ... in the order znote first sees them, so
  "Acme" may be `⟦CUSTOMER_3~1⟧` and "ACME" `⟦CUSTOMER_3~2⟧`. Each token is put back as exactly
  the text it replaced, capitals included.

## Example

```turtle
@prefix skos: <http://www.w3.org/2004/02/skos/core#> .
@prefix znote: <urn:znote:skos#> .
@prefix ex: <https://example.org/terms/> .

ex:terms a skos:ConceptScheme ;
  skos:hasTopConcept ex:customer, ex:project .

ex:customer a skos:Concept ; skos:prefLabel "Customer"@en .
ex:project a skos:Concept ; skos:prefLabel "Project"@en .

ex:example-corp a skos:Concept ;
  skos:prefLabel "Example Corporation"@en ;
  skos:altLabel "Example Corp", "ExCo" ;
  skos:hiddenLabel "EXC-001" ;
  skos:broader ex:customer ;
  skos:related ex:blue-harbour ;
  skos:definition "Maintenance contract, renewed every March."@en .

ex:blue-harbour a skos:Concept ;
  skos:prefLabel "Blue Harbour"@en ;
  skos:broader ex:project .

ex:public-tender a skos:Concept ;
  skos:prefLabel "Public tender"@en ;
  skos:broader ex:project ;
  znote:mask false .
```

With this glossary and a new knowledge file, "ExCo asked about Blue Harbour" reaches the
assistant as "⟦CUSTOMER_1~1⟧ asked about ⟦PROJECT_1⟧". The numbers depend on the order in which znote first
sees each term, and never change afterwards.

znote ships a glossary with the categories Person, Organisation, Customer, Supplier, Project,
Product and Location and no terms in them.
