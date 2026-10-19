# A dotted capitalized name is a qualified-name pattern,
# as a positional item and after `key:`.
case d
when [Direction.North, x]
  a()
when {dir: Direction.North}
  b()
end
