# Assigning a name that is not a variant of an enum says so,
# rather than calling it a variant.
enum Dir
  North
end
# expect-error: `Nope` is not a variant of the enum `Dir`
Dir.Nope = "S"
