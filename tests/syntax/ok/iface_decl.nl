# Method signatures without bodies and field requirements.
interface Shape
  fn area: Number
  fn scale(factor: Number): Self
  async fn load(id)
  name: String
end
