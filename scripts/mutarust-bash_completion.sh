#/bin/sh

# Mutago-compatible Bash completion. GO_FLAGS_COMPLETION triggers option
# candidates from the mutarust binary. return 1 matches the mutago helper so
# Bash does not add default filename completions on top of COMPREPLY.
_mutarust() {
	args=("${COMP_WORDS[@]:1:$COMP_CWORD}")

	local IFS=$'\n'
	COMPREPLY=($(GO_FLAGS_COMPLETION=1 ${COMP_WORDS[0]} "${args[@]}"))
	return 1
}

complete -F _mutarust mutarust
