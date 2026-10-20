struct DiceNode
  sides: Number
end

fn describe(node)
  case node
  # expect-error: a bare struct name in `when` compares with the struct itself; the type test is `when is DiceNode`
  when DiceNode
    "dice"
  else
    "other"
  end
end

print describe(DiceNode { sides: 6 })
