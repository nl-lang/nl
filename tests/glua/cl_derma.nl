# Client: a derma window with a list of players.

fn open_menu
  let frame = VGUI.create("DFrame")
  frame.set_size 400, 300
  frame.center
  frame.make_popup
  frame:SetTitle("Players")

  let list = VGUI.create("DListView", frame)
  list.dock FILL
  list:AddColumn("Name")
  list:AddColumn("Ping")
  for ply in PlayerLib.get_all()
    list:AddLine(ply.nick, ply.ping)
  end

  let close = VGUI.create("DButton", frame)
  close.dock BOTTOM
  close.set_text "Close"
  close.DoClick = fn
    frame.close
    Surface.play_sound "buttons/button14.wav"
  end
end

ConCommand.add "nl_menu", fn(ply: Player, cmd: String, args: Table)
  open_menu
end
