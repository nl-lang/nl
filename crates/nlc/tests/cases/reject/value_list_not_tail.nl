# A value list is a statement only as the last statement of a
# function; anywhere else its values would be discarded.
fn f(): (Number, Number)
  # expect-error: this list is not the last statement, so its values are discarded
  1, 2
  3, 4
end
