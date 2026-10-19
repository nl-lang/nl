# A name exported twice is an error in the
# exporting file (the importer would see one item, the export table the
# other).
pub fn f
  "fn f"
end
# expect-error: `f` is exported twice
pub const f = 5
