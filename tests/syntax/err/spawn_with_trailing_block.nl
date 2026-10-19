# A block cannot be sent to a worker, after
# parenthesized or after command arguments.
t = spawn f(x) do
end
spawn f x do
end
