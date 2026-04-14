Output assets/demo-oxvelte.gif
Set FontSize 22
Set Width 450
Set Height 300
Set Theme "Catppuccin Mocha"
Set Padding 30
Set TypingSpeed 40ms
Set Shell "bash"

Hide
Type `export PS1="\033[1;35m❯\033[0m "` Enter
Type `alias oxvelte="bash demo/run-oxvelte-timed.sh"` Enter
Type "clear" Enter
Show
Sleep 300ms

Type `echo -e "\033[1moxvelte\033[0m"` Enter
Sleep 600ms
Type "oxvelte lint src/"
Sleep 400ms
Enter
Sleep 5s
Sleep 2s
