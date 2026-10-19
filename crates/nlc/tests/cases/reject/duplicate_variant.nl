# An enum variant is declared once.
enum Dir
  North
  # expect-error: `North` is declared twice
  North
end
