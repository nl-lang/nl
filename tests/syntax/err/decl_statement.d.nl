# A declaration file holds `declare` items only; a statement and a body are errors, and parsing goes on
let x = 1
declare fn ok(): Number
declare module M
  fn with_body(): Number
    1
  end
  fn no_type(a)
end
