pub struct Piece
  rank: Number
  name: String = "pawn"
  under: Piece?
end
pub struct Crowned : Piece
  crown: Bool = true
end
