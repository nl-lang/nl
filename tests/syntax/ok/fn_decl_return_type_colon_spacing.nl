# After an fn header a `:` that has no whitespace before
# it, or has whitespace after it, is the return-type colon.
fn f(): T
end
fn g() : T
end
fn h(x) : T
end
