# The expression of an interpolation is in bracket
# context; in a heredoc it may span lines.
s = """
  #{a
    - b}
"""
