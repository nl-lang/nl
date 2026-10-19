# Rejected by the compiler.
fn main
  for i in 1..3
    let t = spawn do
      # expect-error: `break` cannot cross into a `spawn do` block
      break
    end
  end
end
