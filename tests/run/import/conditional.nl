# Helper of import_pub_conditional: pub items in the branches of file-level ifs
let fancy = true

if fancy
  pub fn render(s)
    "*#{s}*"
  end

  pub const STYLE = "fancy"

  pub module Theme
    fn color
      "gold"
    end
  end
else
  # the same names in another branch are the same exports
  pub fn render(s)
    s
  end

  pub const STYLE = "plain"
end

unless fancy
  pub fn debug_dump
    "never defined"
  end
end

# the file's own code sees them too, after the if
pub fn describe
  "#{STYLE} #{render("text")}"
end
