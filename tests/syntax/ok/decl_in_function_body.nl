# Declarations may appear in any block, not only at file level.
fn outer
  struct Local
    x: Number
  end
  fn inner end
  const k = 1
  enum E end
  type T = Number
end
