# Recovery: a missing `end` deep inside mixed constructs, in a function
# argument; the outer constructs still close.
fn outer()
  items.each do |x|
    case x
    when 1
      if y
        a()
    end
  end
end
z = 1
