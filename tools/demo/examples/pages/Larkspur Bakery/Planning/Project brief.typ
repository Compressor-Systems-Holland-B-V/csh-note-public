#set page(paper: "a4", numbering: "1")
#set par(justify: true)
#set heading(numbering: "1.1")

#align(center)[
  #text(22pt, weight: "bold")[Ordering app]
  #v(-6pt)
  #text(12pt, fill: gray)[Project brief · Larkspur Bakery]
]

= Goal

Let customers order bread and pastries the evening before and pick them up
from 7:00 at any of our four stores. Fewer unsold loaves, shorter queues.

= Scope

- Order and pay in the app, pick up in store.
- One menu per store, updated by the store lead before 16:00.
- No delivery in the first release.

= Budget

#table(
  columns: (1fr, auto),
  inset: 8pt,
  align: (left, right),
  table.header([*Item*], [*EUR*]),
  [Design and build], [38,000],
  [Payment provider, first year], [4,200],
  [Tablets for four stores], [2,400],
  [*Total*], [*44,600*],
)

= Waste estimate

With $n$ loaves baked and $d$ sold, waste is $w = (n - d) / n$.
Pre-orders let us bake closer to demand, so we expect

$ w_"new" approx 0.6 dot w_"now" $

within three months of launch.
