# Where a word can only be a field name, `true`,
# `false` and `nil` are plain names: struct and interface fields, struct
# literal fields, struct pattern fields, record type fields, `.name`, `@name`.
struct Flags
  true: Bool
  nil = 0
end
interface HasFalse
  false: Bool
end
f = Flags { true: 1, nil: 2 }
let Flags {true: t, false: u} = f
let r: {true: Number, nil: String} = v
x = f.nil + f.true + @false
