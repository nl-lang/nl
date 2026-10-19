# A variable named `then` can still be passed in a header with
# parentheses; outside headers `then` is an ordinary name. Here the `then`
# without parentheses is the error.
if check(then)
  then = 1
end
if check then
end
