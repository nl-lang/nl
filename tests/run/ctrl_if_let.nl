# if let and elseif let (may-match)
let roles = {ann: "admin", bob: "user"}
fn find(name)
  roles[name]
end

if let role = find("ann")
  print "ann is #{role}"
end

if let role = find("zed")
  print "never"
else
  print "zed not found"
end

if let flag = false
  print "false is not nil, so it binds: #{flag}"
end

let user = {role: "admin", name: "kim"}
if let {role: "user", name} = user
  print "plain user #{name}"
elseif let {role: "admin", name} = user
  print "admin #{name}"
end

if let {name, age} = user
  print "never: age is missing"
else
  print "a missing key does not match"
end

if let [a, b] = [1, 2]
  print "pair #{a} #{b}"
end
