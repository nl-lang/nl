# To<T> and From<T> conversions, tried after `is` and before the built-in ones
struct Celsius
  degrees: Number
end

interface To<String> for Celsius
  fn to: String
    "#{@degrees}C"
  end
end

interface From<Number> for Celsius
  fn from(val: Number): Self
    Celsius { degrees: val }
  end
end

let c = Celsius { degrees: 21 }
print(c as String)
print tostring(c)
print "it is #{c}"
let made = 30 as Celsius
print typeof(made)
print made.degrees
print Celsius.from(5).degrees
print((c as Celsius) == c)

struct Meters
  value: Number
end

struct Feet
  value: Number
end

interface To<Feet> for Meters
  fn to: Feet
    Feet { value: @value * 3 }
  end
end

interface From<Meters> for Feet
  fn from(val: Meters): Self
    Feet { value: -1 }
  end
end

let f = Meters { value: 2 } as Feet
print f.value
let plain: any = "x"
try
  let m = plain as Meters
catch err: ConversionError
  print "no conversion: ConversionError"
end
