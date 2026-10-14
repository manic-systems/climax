// SPDX-License-Identifier: EUPL-1.2

//! the per-shell halves of the completion protocol
//!
//! a shell's script runs `COMPLETE=<shell> <program> -- <words>` on TAB, with
//! the word under the cursor last. the program answers on stdout. for bash,
//! zsh and fish the first line is the [`Fallback`]'s header word, and each
//! further line is one candidate. nu
//! takes a JSON list of `{value, description}` records, or `null` to fall back
//! to its own file completion. a shell that can drop an empty last word also
//! sets `COMPLETE_INDEX`, the position of the word under the cursor among those
//! after the `--`.

use core::{
    error::Error,
    fmt::{
        self,
        Write as _,
    },
    str::FromStr,
};

use crate::engine::{
    Completions,
    Fallback,
};

/// a shell caress can complete for
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Shell {
    Bash,
    Zsh,
    Fish,
    Nu,
    Elvish,
    PowerShell,
}

/// a shell name caress does not know
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnknownShell(pub String);

impl fmt::Display for UnknownShell {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "unknown shell '{}', expected bash, zsh, fish, nu, elvish or powershell",
            self.0
        )
    }
}

impl Error for UnknownShell {}

impl FromStr for Shell {
    type Err = UnknownShell;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "bash" => Ok(Self::Bash),
            "zsh" => Ok(Self::Zsh),
            "fish" => Ok(Self::Fish),
            "nu" => Ok(Self::Nu),
            "elvish" => Ok(Self::Elvish),
            "powershell" => Ok(Self::PowerShell),
            _ => Err(UnknownShell(s.to_owned())),
        }
    }
}

/// `_describe` reads `value:help`, so a `:` in the value and the `\` that
/// escapes it are escaped
fn zsh_escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace(':', "\\:")
}

impl fmt::Display for Shell {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Bash => "bash",
            Self::Zsh => "zsh",
            Self::Fish => "fish",
            Self::Nu => "nu",
            Self::Elvish => "elvish",
            Self::PowerShell => "powershell",
        })
    }
}

impl Shell {
    /// the script that registers completion for `program` in this shell
    ///
    /// bash, fish and elvish source it, zsh autoloads it from `fpath` as
    /// `_<program>` or sources it, nu sources it from its config, and
    /// PowerShell dot-sources it from its profile.
    #[must_use]
    pub fn script(self, program: &str) -> String {
        let ident = program
            .chars()
            .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
            .collect::<String>();
        let template = match self {
            Self::Bash => BASH,
            Self::Zsh => ZSH,
            Self::Fish => FISH,
            Self::Nu => NU,
            Self::Elvish => ELVISH,
            Self::PowerShell => POWERSHELL,
        };
        template
            .replace("@IDENT@", &ident)
            .replace("@PROGRAM@", program)
            .replace("@FILES@", Fallback::Files.header())
    }

    /// the program's answer, in the form this shell's script reads
    #[must_use]
    pub fn render(self, completions: &Completions) -> String {
        if self == Self::Nu {
            return nu_json(completions);
        }

        let mut out = format!("{}\n", completions.fallback.header());
        for candidate in &completions.candidates {
            let value = &candidate.value;
            let help = candidate.help.lines().next().unwrap_or_default();
            match self {
                Self::Zsh if help.is_empty() => out.push_str(&zsh_escape(value)),
                Self::Zsh => {
                    out.push_str(&zsh_escape(value));
                    out.push(':');
                    out.push_str(help);
                },
                Self::Fish | Self::Elvish | Self::PowerShell if !help.is_empty() => {
                    out.push_str(value);
                    out.push('\t');
                    out.push_str(help);
                },
                Self::Bash | Self::Fish | Self::Nu | Self::Elvish | Self::PowerShell => {
                    out.push_str(value);
                },
            }
            out.push('\n');
        }
        out
    }
}

impl Fallback {
    /// the first line of a bash, zsh or fish answer, which their scripts
    /// compare against
    const fn header(self) -> &'static str {
        match self {
            Self::Nothing => "values",
            Self::Files => "files",
        }
    }
}

fn nu_json(completions: &Completions) -> String {
    if completions.fallback == Fallback::Files && completions.candidates.is_empty() {
        return "null".to_owned();
    }

    let records = completions
        .candidates
        .iter()
        .map(|candidate| {
            format!(
                "{{\"value\":{},\"description\":{}}}",
                json_string(&candidate.value),
                json_string(candidate.help.lines().next().unwrap_or_default())
            )
        })
        .collect::<Vec<_>>();
    format!("[{}]", records.join(","))
}

fn json_string(text: &str) -> String {
    let mut out = String::from('"');
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            _ if ch.is_control() => {
                let _write_result = write!(out, "\\u{:04x}", u32::from(ch));
            },
            _ => out.push(ch),
        }
    }
    out.push('"');
    out
}

// bash splits words at `=` and `:` too, so the word it replaces can be a tail
// of the one caress completed, and each candidate is trimmed to match
const BASH: &str = r#"_caress_@IDENT@_words() {
    local line=$1 word= quote= started= c i
    words=()
    for ((i = 0; i < ${#line}; i++)); do
        c=${line:i:1}
        if [[ $quote == "'" ]]; then
            if [[ $c == "'" ]]; then quote=; else word+=$c; fi
        elif [[ $quote == '"' ]]; then
            if [[ $c == '"' ]]; then
                quote=
            elif [[ $c == '\' && ${line:i+1:1} == [\"\\\$\`] ]]; then
                word+=${line:i+1:1}
                i=$((i + 1))
            else
                word+=$c
            fi
        elif [[ $c == [[:space:]] ]]; then
            [[ $started ]] && words+=("$word")
            word= started=
        else
            started=1
            if [[ $c == "'" || $c == '"' ]]; then
                quote=$c
            elif [[ $c == '\' ]]; then
                word+=${line:i+1:1}
                i=$((i + 1))
            else
                word+=$c
            fi
        fi
    done
    words+=("$word")
}

_caress_@IDENT@() {
    local -a words
    local raw=${COMP_WORDS[COMP_CWORD]}
    [[ $raw =~ ^[=:]+$ ]] && raw=
    local edge=${COMP_LINE:COMP_POINT-${#raw}-1:1}
    [[ $edge == [@\$] && $COMP_WORDBREAKS == *"$edge"* ]] && raw=$edge$raw
    _caress_@IDENT@_words "$raw"
    local cur=${words[-1]}
    _caress_@IDENT@_words "${COMP_LINE:0:COMP_POINT}"
    local typed=${words[-1]}
    local lead=${typed%"$cur"}

    local -a out
    mapfile -t out < <(COMPLETE=bash "${words[0]}" -- "${words[@]:1}" 2>/dev/null)
    local candidate value
    COMPREPLY=()
    for candidate in "${out[@]:1}"; do
        value=${candidate#"$lead"}
        case $raw in
            \'*) value=${value//\'/\'\\\'\'} ;;
            \"*)
                value=${value//\\/\\\\}
                value=${value//\"/\\\"}
                value=${value//\$/\\\$}
                value=${value//\`/\\\`}
                ;;
            *) printf -v value %q "$value" ;;
        esac
        COMPREPLY+=("$value")
    done
    if (( ${#COMPREPLY[@]} == 1 )) && [[ ${COMPREPLY[0]} == */ ]]; then
        compopt -o nospace
    fi
    if [[ ${out[0]} == @FILES@ ]]; then
        compopt -o filenames
        mapfile -t -O "${#COMPREPLY[@]}" COMPREPLY < <(compgen -f -- "$cur")
    fi
}
complete -F _caress_@IDENT@ @PROGRAM@
"#;

const ZSH: &str = r#"#compdef @PROGRAM@

_caress_@IDENT@() {
    local -a out candidates dirs
    out=("${(@f)$(COMPLETE=zsh ${words[1]} -- "${(@Q)words[2,CURRENT-1]}" "${(Q)PREFIX}" 2>/dev/null)}")
    candidates=("${(@)out[2,-1]}")
    dirs=("${(@M)candidates:#*/}")
    candidates=("${(@)candidates:#*/}")
    (( ${#candidates} )) && _describe -t values @PROGRAM@ candidates -M 'm:{a-zA-Z}={A-Za-z}'
    (( ${#dirs} )) && _describe -t values @PROGRAM@ dirs -S ''
    [[ ${out[1]} == @FILES@ ]] && _files
}

if [[ $zsh_eval_context[-1] == loadautofunc ]]; then
    _caress_@IDENT@ "$@"
else
    compdef _caress_@IDENT@ @PROGRAM@
fi
"#;

const FISH: &str = r#"function __caress_@IDENT@
    set -l tokens (commandline -opc) (commandline -ct | string unescape; or commandline -ct)
    set -l out (COMPLETE=fish $tokens[1] -- $tokens[2..] 2>/dev/null)
    printf '%s\n' $out[2..]
    if test "$out[1]" = @FILES@
        __fish_complete_path (commandline -ct)
    end
end
complete -c @PROGRAM@ -f -a '(__caress_@IDENT@)'
"#;

const NU: &str = r#"$env.config.completions.external.enable = true
$env.config.completions.external.completer = do {
    let previous = $env.config.completions.external.completer?
    {|spans|
        if ($spans | first) == "@PROGRAM@" {
            COMPLETE=nu ^@PROGRAM@ -- ...($spans | skip 1) | from json
        } else if $previous != null {
            do $previous $spans
        }
    }
}
"#;

const ELVISH: &str = r#"use str
set edit:completion:arg-completer[@PROGRAM@] = {|@words|
    var out = [(env COMPLETE=elvish $words[0] -- (all $words[1..]) 2>/dev/null)]
    if (== (count $out) 0) {
        return
    }
    for line $out[1..] {
        var parts = [(str:split &max=2 "\t" $line)]
        var display = $parts[0]
        if (> (count $parts) 1) {
            set display = $parts[0]'  '$parts[1]
        }
        var suffix = ' '
        if (str:has-suffix $parts[0] /) {
            set suffix = ''
        }
        edit:complex-candidate $parts[0] &display=$display &code-suffix=$suffix
    }
    if (eq $out[0] @FILES@) {
        edit:complete-filename $words[-1]
    }
}
"#;

// PowerShell before 7.3 drops an empty argument to a native program, so the
// script says where the word under the cursor sits instead of relying on it
const POWERSHELL: &str = r#"Register-ArgumentCompleter -Native -CommandName '@PROGRAM@' -ScriptBlock {
    param($wordToComplete, $commandAst, $cursorPosition)
    $words = @($commandAst.CommandElements |
        Where-Object { $_.Extent.StartOffset -lt $cursorPosition } |
        ForEach-Object {
            if ($_ -is [System.Management.Automation.Language.StringConstantExpressionAst]) {
                $_.Value
            } else {
                $_.Extent.Text
            }
        })
    if ($wordToComplete -eq '') {
        $words += ''
    }
    $env:COMPLETE = 'powershell'
    $env:COMPLETE_INDEX = $words.Count - 2
    $line = @('--') + @($words | Select-Object -Skip 1)
    $out = @(& $words[0] @line 2>$null)
    Remove-Item Env:COMPLETE, Env:COMPLETE_INDEX
    if ($out.Count -eq 0) {
        return
    }
    $out | Select-Object -Skip 1 | ForEach-Object {
        $value, $help = $_ -split "`t", 2
        if (-not $help) {
            $help = $value
        }
        $text = $value
        if ($value -match '[\s''"$`(){};,|&@#<>]') {
            $text = "'" + ($value -replace "'", "''") + "'"
        }
        [System.Management.Automation.CompletionResult]::new($text, $value, 'ParameterValue', $help)
    }
}
"#;
