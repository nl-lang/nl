# A heredoc cannot be opened inside an interpolation.
a = """
  #{ """
  x
  """ }
"""
b = 1
