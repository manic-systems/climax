# pound-derive-impl

The expansion behind pound's derive macros. It is a plain library so that
`pound-derive` and `climax-derive` can each root the generated code at their own
runtime path.

This crate is an implementation detail of those two and carries no stability
promise of its own.
