use std/assert

def successful [result: record, label: string] {
    assert equal $result.exit_code 0 $"($label) failed with ($result.stderr)"
    $result.stdout
}

def candidates [shell: string, script: path, words: list<string>] {
    let result = if $shell == bash {
        let driver = r#'set -e
source "$1"
shift
COMP_WORDS=(command_docs "$@")
COMP_CWORD=$((${#COMP_WORDS[@]} - 1))
_pound_command_docs_complete
if ((${#COMPREPLY[@]}))
then
    printf '%s\n' "${COMPREPLY[@]}"
fi
'#
        ^bash --noprofile --norc -c $driver pound-smoke $script ...$words | complete
    } else {
        let driver = r#'source "$argv[1]"
or exit $status
complete -C "$argv[2]"
'#
        let commandline = [command_docs ...$words] | str join ' '
        ^fish --no-config -c $driver $script $commandline | complete
    }
    successful $result $"($shell) completion"
    | lines
    | where {|line| $line != '' }
    | each {|line| $line | split row (char tab) | first }
    | sort
}

def smoke [binary: path, scratch: path] {
    let cases = [
        {words: ['--f'], expected: ['--format']}
        {words: [cache cl], expected: [clean]}
        {words: [cache ls --format j], expected: [json]}
        {words: [cache ls --output-format t], expected: [text toml]}
        {words: [fetch -- --f], expected: []}
    ]
    for shell in [bash fish] {
        let script = $scratch | path join $"completions.($shell)"
        let generated = ^$binary completions $shell | complete
        successful $generated $"generate ($shell) script" | save $script
        for case in $cases {
            let actual = candidates $shell $script $case.words
            assert equal $actual $case.expected $"($shell) completion for ($case.words | str join ' ')"
        }
        let inline = if $shell == bash {
            {words: [cache ls --format '=' j], expected: [json]}
        } else {
            {words: [cache ls '--format=j'], expected: ['--format=json']}
        }
        assert equal (candidates $shell $script $inline.words) $inline.expected $"($shell) inline value completion"
        let after_inline = if $shell == bash {
            [--format '=' json ca]
        } else {
            ['--format=json' ca]
        }
        assert equal (candidates $shell $script $after_inline) [cache] $"($shell) completion after inline value"
        let root = candidates $shell $script ['']
        assert ('--trace' not-in $root) $"($shell) exposed a hidden flag"
        assert ('__complete' not-in $root) $"($shell) exposed its internal command"
        let cache = candidates $shell $script [cache '']
        assert ('doctor' not-in $cache) $"($shell) exposed a hidden command"
    }

    let help = successful (^$binary help fetch | complete) 'custom help'
    for expected in [
        '<URL>'
        '--retries'
        'POUND_DEMO_RETRIES'
        '--output-format'
        '[choices text, json, toml]'
    ] {
        assert ($help | str contains $expected) $"custom help omitted ($expected)"
    }
    assert (not ($help | str contains '--trace')) 'custom help exposed a hidden flag'

    let parsed = successful (
        ^$binary get https://example.invalid --format json -vv --retries 5 | complete
    ) 'valid download request'
    for expected in [Fetch Json 'verbose: 2' 'retries: 5'] {
        assert ($parsed | str contains $expected) $"parsed request omitted ($expected)"
    }

    for args in [
        [completions zsh]
        [help missing]
        [get https://example.invalid --retries nope]
    ] {
        let rejected = ^$binary ...$args | complete
        assert equal $rejected.exit_code 2 $"invalid input was accepted for ($args | str join ' ')"
        assert ($rejected.stderr | is-not-empty) 'invalid input produced no diagnostic'
    }
}

def main [] {
    for dependency in [cargo bash fish] {
        assert (which $dependency | is-not-empty) $"missing ($dependency) in PATH"
    }
    cd ($env.FILE_PWD | path join ../../.. | path expand)
    hide-env --ignore-errors POUND_DEMO_RETRIES
    successful (^cargo build -p pound --example command_docs | complete) 'build example' | ignore
    let binary = 'target/debug/examples/command_docs' | path expand
    let scratch = mktemp --directory --tmpdir pound-command-docs.XXXXXXXX
    try {
        smoke $binary $scratch
    } catch {|failure|
        rm --recursive --force $scratch
        error make $failure
    }
    rm --recursive --force $scratch
    print 'Custom help, parsing, and Bash and Fish completions passed'
}
