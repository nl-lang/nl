# `pub` directly in a branch of an `if` or `unless` statement at file
# level: an export that exists where the branch runs.
if CLIENT
  pub fn draw(text)
    print text
  end
  pub const FONT = "Default"
elseif SERVER
  pub module Net
  end
else
  pub struct Stub
    x: Number
  end
end
unless SERVER
  pub enum Mode
    On
  end
end
