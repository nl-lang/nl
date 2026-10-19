# A failed must-match pattern raises MatchError; a missing key binds nil
fn attempt(label: String, f: Function)
  try
    f()
    print "#{label}: matched"
  catch err: MatchError
    print "#{label}: MatchError"
  end
end

let three: any = [1, 2, 3]
let two: any = [1, 2]
let stop: any = {kind: "stop"}
let number: any = 5
let empty: any = {}

attempt "exact length" do
  let [a, b] = two
end
attempt "too long" do
  let [a, b] = three
end
attempt "rest" do
  let [a, *rest] = three
end
attempt "literal" do
  let {kind: "move"} = stop
end
attempt "not a table" do
  let {kind} = number
end
attempt "range" do
  let [1..2, *] = three
end
attempt "at least one" do
  let [a, *] = empty
end
attempt "missing key is nil" do
  let {absent} = stop
end

fn takes_pair([a, b])
  a + b
end

print takes_pair(two)
try
  takes_pair(three)
catch err: MatchError
  print "parameter: MatchError"
end
