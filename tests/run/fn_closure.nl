# Closures capture locals; a discarded variable is called
fn make_counter
  let n = 0
  let bump = fn()
    n += 1
    n
  end
  bump!
end

let c = make_counter()
print c()
print c()
c
print c()
let other = make_counter()
print other()
