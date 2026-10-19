# `raise err` inside a catch re-raises the same exception
let first = nil
try
  try
    raise "original"
  catch err
    first = err
    raise err
  end
catch again
  print again.message
  print(again == first)
end
