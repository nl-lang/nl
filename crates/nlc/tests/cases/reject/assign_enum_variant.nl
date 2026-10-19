# A variant is a constant of its enum.
enum Dir
  North
end
# expect-error: variants cannot be assigned
Dir.North = "S"
