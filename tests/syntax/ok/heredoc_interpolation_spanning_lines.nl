# In a heredoc an interpolation may span lines.
s = """
  total: #{
    a +
    b
  }
"""
