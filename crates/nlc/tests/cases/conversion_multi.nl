# `To<T>` and `From<T>` keep their type
# argument, so a type implements them for several arguments. `as` picks by
# the target type and by the source value's type; `T.from(v)` dispatches
# the same way; `x.to` works while there is one `To` and raises when there
# are several, as Rust's `x.into()` needs an annotation then.
import { Feet } from 'lib/conv_feet'

struct Base
  v: Number
end
struct Out
  v: Number
end

interface To<Out> for Base
  fn to: Out
    Out { v: @v * 10 }
  end
end
let b = Base { v: 2 }
print b.to.v                      # one To: fine

interface To<String> for Base
  fn to: String
    "Base(#{@v})"
  end
end
print((b as Out).v)
print(b as String)
print "#{b}"
try
  print b.to
catch e: ConversionError
  print "to: #{e.message}"
end

# Implementing the same To<U> again replaces it.
interface To<Out> for Base
  fn to: Out
    Out { v: @v * 100 }
  end
end
print((b as Out).v)

# A child uses its parent's conversions; its own come first.
struct Child : Base
end
let c = Child { v: 3 }
print((c as Out).v)
interface To<Out> for Child
  fn to: Out
    Out { v: -1 }
  end
end
print((c as Out).v)
print(c as String)

# Several From<S> on one target: the value's own type wins, then the
# nearest ancestor, then the one interface that accepts it.
interface Named
  fn name: String
end
interface Sized
  fn size: Number
end
struct Target
  via: String
end
interface From<Base> for Target
  fn from(val: Base): Self
    Target { via: "Base" }
  end
end
interface From<Child> for Target
  fn from(val: Child): Self
    Target { via: "Child" }
  end
end
interface From<Named> for Target
  fn from(val: Named): Self
    Target { via: "Named" }
  end
end
interface From<Sized> for Target
  fn from(val: Sized): Self
    Target { via: "Sized" }
  end
end
struct Grand : Child
end
print((b as Target).via)
print((c as Target).via)
print((Grand { v: 1 } as Target).via)
print Target.from(c).via

struct Thing
  n: Number
end
interface Named for Thing
  fn name: String
    "thing"
  end
end
print((Thing { n: 1 } as Target).via)
interface Sized for Thing
  fn size: Number
    @n
  end
end
try
  print(Thing { n: 1 } as Target)
catch e: ConversionError
  print "as: #{e.message}"
end
try
  print Target.from(Thing { n: 1 })
catch e: ConversionError
  print "from: #{e.message}"
end
try
  print Target.from(42)
catch e: ConversionError
  print "from: #{e.message}"
end

# Primitive types: a second To from another file shares the dispatcher.
struct Celsius
  degrees: Number
end
interface To<Celsius> for Number
  fn to: Celsius
    Celsius { degrees: self }
  end
end
print((7 as Celsius).degrees)
print((7 as Feet).value)
try
  print 7.to
catch e: ConversionError
  print "to: #{e.message}"
end
interface From<String> for Number
  fn from(val: String): Self
    val.len
  end
end
interface From<Bool> for Number
  fn from(val: Bool): Self
    if val
      1
    else
      0
    end
  end
end
print Number.from("abcd")
print Number.from(true)
print(false as Number)
