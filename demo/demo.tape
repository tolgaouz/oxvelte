Output assets/demo.gif
Set FontSize 22
Set Width 900
Set Height 500
Set Theme "Catppuccin Mocha"
Set Padding 40
Set TypingSpeed 40ms
Set Shell "bash"

Hide
Type `export PS1="\033[1;35m❯\033[0m "` Enter
Type `alias eslint="bash demo/run-eslint.sh"` Enter
Type `alias oxvelte="bash demo/run-oxvelte.sh"` Enter
Type "clear" Enter
Show
Sleep 500ms

Type `echo "shadcn-svelte  ·  1,603 Svelte files"` Enter
Sleep 800ms
Type "" Enter

# ESLint run
Type "eslint src/"
Sleep 500ms
Enter
Sleep 5s

Type "" Enter

# Oxvelte run
Type "oxvelte lint src/"
Sleep 500ms
Enter
Sleep 3s

Sleep 3s
