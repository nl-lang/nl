# Type arguments in an implementation's interface.
interface To<String> for Celsius
  fn to: String
    "#{@deg}C"
  end
end
interface Lib.Show for Lib.Point
end
