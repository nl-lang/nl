# Built-in conversions with `as`, and ConversionError
print(5 as String)
print(typeof(5 as String))
print("42" as Number)
print(typeof("42" as Number))
print(1.5 as String)
print(true as String)
print(nil as Bool)
print(0 as Bool)
print("" as Bool)
print(false as Bool)
print("same" as String)

let word: any = "abc"
try
  let n = word as Number
catch err: ConversionError
  print "abc as Number: ConversionError"
end

let tbl: any = {}
try
  let n = tbl as Number
catch err: ConversionError
  print "table as Number: ConversionError"
end

let num: any = 5
try
  let t = num as Table
catch err: ConversionError
  print "number as Table: ConversionError"
end
