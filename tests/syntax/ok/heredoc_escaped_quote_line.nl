# A content line that starts with `"""` escapes the first
# quote; a `"""` that is not at the start of its line is text.
s = """
  \"""
  text """ inside
"""
