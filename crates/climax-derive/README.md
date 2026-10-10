# climax-derive

The `Parse` and `ValueEnum` derives from pound, rooted at `::climax::pound` so
an application that depends only on `climax` can use them.

Enable the `derive` feature on `climax` instead of depending on this crate
directly.

With the `derive` and `structured` features it also provides the
`#[climax::serde(Serialize, Deserialize)]` attribute, which derives serde's
traits rooted at `::climax::serde` and keeps the item's other attributes.
