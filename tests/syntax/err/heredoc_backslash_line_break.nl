# In a heredoc the line break is already part of the text, so
# a backslash directly before it is an error.
let s = """
  one \
  two
  """
x = 1
