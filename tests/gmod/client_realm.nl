# In the client realm, gmod's SERVER and CLIENT are known when the file is
# compiled: code under `if SERVER` is left out, so it may use what the
# client does not have, and the branches the realm always takes run alone.
if SERVER
  :util.AddNetworkString("never")
  IO.write("never")
elseif CLIENT
  print "if: client branch"
else
  IO.write("never")
end

if CLIENT
  print "if CLIENT"
else
  :util.AddNetworkString("never")
end

unless SERVER
  print "unless SERVER"
else
  IO.write("never")
end

unless CLIENT
  IO.write("never")
else
  print "unless CLIENT: else"
end

print "modifier if CLIENT" if CLIENT
IO.write("never") if SERVER
IO.write("never") unless CLIENT
print "modifier unless SERVER" unless SERVER
IO.write("never") if SERVER and :util.AddNetworkString("never")
print "not SERVER or anything" if not SERVER or :never()

# As values.
let realm = if SERVER
  :util.AddNetworkString("never")
  "server"
else
  "client"
end
print "realm: #{realm}"
let nothing = if SERVER
  "server"
end
print "nothing: #{nothing}"

fn which
  if CLIENT
    return "returned from the client branch"
  end
  "never"
end
print which()

# A condition that is not decided stays.
let n = 2
if n > 1 and CLIENT
  print "runtime condition"
end
