# climax-derive

pound's `Parse` and `ValueEnum` derives, rooted at `::climax::pound` so an application that
depends only on `climax` can use them.

enable the `derive` feature on `climax` rather than depending on this crate

with `derive` and `structured` it also provides `#[climax::serde(Serialize, Deserialize)]`, which
derives serde's traits rooted at `::climax::serde` and keeps the item's other attributes
