//! The terminal's shell, started so that its prompt marks itself (OSC
//! 133: `A` where the prompt begins, `B` where what is typed begins, `C`
//! when a command begins), as VS Code's terminal starts one. The terminal
//! edits what is typed as a field's text is edited, across every line of
//! it, and the marks are how it knows where that begins. Taken from
//! Emaki's. Nothing else changes: each of the person's own startup files
//! runs as it would, and the marks are put round the prompt last.
//!
//! zsh reads its startup files from a folder of ours (`ZDOTDIR`), each of
//! which runs the person's own of the same name first; bash reads one
//! file of ours in place of its login files, which it runs as a login
//! shell would; fish runs one of ours after its own configuration. Any
//! other shell, and PowerShell on Windows, starts as it always did, with
//! no marks.

use crate::paths;
use std::path::{Path, PathBuf};

const ZSHENV: &str = r#"# Onyx's terminal: zsh reads this in place of the person's own,
# which runs first. Written by Onyx.
ONYX_ZDOTDIR=$ZDOTDIR
ZDOTDIR=$ONYX_USER_ZDOTDIR
[[ -f $ZDOTDIR/.zshenv ]] && builtin source $ZDOTDIR/.zshenv
ONYX_USER_ZDOTDIR=$ZDOTDIR
ZDOTDIR=$ONYX_ZDOTDIR
"#;

const ZPROFILE: &str = r#"# Onyx's terminal: the person's own .zprofile. Written by Onyx.
ZDOTDIR=$ONYX_USER_ZDOTDIR
[[ -f $ZDOTDIR/.zprofile ]] && builtin source $ZDOTDIR/.zprofile
ONYX_USER_ZDOTDIR=$ZDOTDIR
ZDOTDIR=$ONYX_ZDOTDIR
"#;

const ZSHRC: &str = r#"# Onyx's terminal: the person's own .zshrc, then the prompt's
# marks. Written by Onyx.
# The system's zshrc names the history after the folder zsh started in,
# which is ours: the person's history is theirs.
[[ $HISTFILE == $ONYX_ZDOTDIR/.zsh_history ]] && HISTFILE=$ONYX_USER_ZDOTDIR/.zsh_history
ZDOTDIR=$ONYX_USER_ZDOTDIR
[[ -f $ZDOTDIR/.zshrc ]] && builtin source $ZDOTDIR/.zshrc
ONYX_USER_ZDOTDIR=$ZDOTDIR
ZDOTDIR=$ONYX_ZDOTDIR
builtin source $ONYX_ZDOTDIR/onyx.zsh
"#;

const ZLOGIN: &str = r#"# Onyx's terminal: the person's own .zlogin, and from here on the
# session is theirs. Written by Onyx.
ZDOTDIR=$ONYX_USER_ZDOTDIR
[[ -f $ZDOTDIR/.zlogin ]] && builtin source $ZDOTDIR/.zlogin
[[ -z $ONYX_HAD_ZDOTDIR && $ZDOTDIR == $HOME ]] && unset ZDOTDIR
unset ONYX_ZDOTDIR ONYX_USER_ZDOTDIR ONYX_HAD_ZDOTDIR
"#;

const ZSH_MARKS: &str = r#"# The prompt's marks for Onyx's terminal (OSC 133). Put round the
# prompt at every prompt and after every other hook, so a theme that sets
# the prompt each time keeps them. Written by Onyx.
_onyx_marks() {
  local a=$'%{\e]133;A\a%}' b=$'%{\e]133;B\a%}'
  PS1=${PS1//$a/}
  PS1=${PS1//$b/}
  PS1=$a$PS1$b
}
_onyx_ran() { builtin print -n $'\e]133;C\a' }
autoload -Uz add-zsh-hook
add-zsh-hook precmd _onyx_marks
add-zsh-hook preexec _onyx_ran
"#;

const BASH: &str = r#"# Onyx's terminal: bash reads this in place of its login files,
# which run as a login shell runs them, then the prompt's marks (OSC 133).
# Written by Onyx.
[ -r /etc/profile ] && . /etc/profile
if [ -r ~/.bash_profile ]; then . ~/.bash_profile
elif [ -r ~/.bash_login ]; then . ~/.bash_login
elif [ -r ~/.profile ]; then . ~/.profile
fi
__onyx_marks() {
  local a='\[\e]133;A\a\]' b='\[\e]133;B\a\]'
  PS1=${PS1//"$a"/}
  PS1=${PS1//"$b"/}
  PS1="$a$PS1$b"
}
PROMPT_COMMAND="${PROMPT_COMMAND:+$PROMPT_COMMAND
}__onyx_marks"
"#;

const FISH: &str = r#"# The prompt's marks for Onyx's terminal (OSC 133), round the
# person's own prompt. Written by Onyx.
functions -q fish_prompt; and functions -c fish_prompt __onyx_fish_prompt
function fish_prompt
    printf '\e]133;A\a'
    functions -q __onyx_fish_prompt; and __onyx_fish_prompt
    printf '\e]133;B\a'
end
function __onyx_ran --on-event fish_preexec
    printf '\e]133;C\a'
end
"#;

/// A file of ours, written when it is not already as it should be.
fn put(path: &Path, text: &str) -> Option<()> {
    if std::fs::read_to_string(path).ok().as_deref() != Some(text) {
        std::fs::create_dir_all(path.parent()?).ok()?;
        std::fs::write(path, text).ok()?;
    }
    Some(())
}

fn folder() -> PathBuf {
    paths::root().join("shell")
}

/// The shell to start in the terminal, as its arguments and what it
/// adds to the environment, so that its prompt marks itself. A shell we
/// do not know, or files that could not be written, start it as before.
pub fn shell_command() -> (Vec<String>, Vec<(String, String)>) {
    let plain = (crate::pty::shell_argv(), Vec::new());
    if cfg!(windows) {
        return plain;
    }
    let shell = plain.0[0].clone();
    let name = Path::new(&shell).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let dir = folder();
    match name.as_str() {
        "zsh" => {
            let z = dir.join("zsh");
            let wrote = [(".zshenv", ZSHENV), (".zprofile", ZPROFILE), (".zshrc", ZSHRC), (".zlogin", ZLOGIN), ("onyx.zsh", ZSH_MARKS)].iter().all(|(f, t)| put(&z.join(f), t).is_some());
            if !wrote {
                return plain;
            }
            let theirs = std::env::var("ZDOTDIR").ok().filter(|d| !d.is_empty());
            let home = std::env::var("HOME").unwrap_or_default();
            let mut env = vec![("ZDOTDIR".to_string(), z.to_string_lossy().to_string()), ("ONYX_USER_ZDOTDIR".to_string(), theirs.clone().unwrap_or(home))];
            if theirs.is_some() {
                env.push(("ONYX_HAD_ZDOTDIR".into(), "1".into()));
            }
            (plain.0, env)
        }
        "bash" => {
            let f = dir.join("onyx.bash");
            match put(&f, BASH) {
                Some(()) => (vec![shell, "--init-file".into(), f.to_string_lossy().to_string()], Vec::new()),
                None => plain,
            }
        }
        "fish" => {
            let f = dir.join("onyx.fish");
            match put(&f, FISH) {
                Some(()) => (vec![shell, "-l".into(), "--init-command".into(), format!("source '{}'", f.to_string_lossy().replace('\'', "\\'"))], Vec::new()),
                None => plain,
            }
        }
        _ => plain,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each file a shell reads is one it can read: a slip in one would
    /// break every shell the terminal opens. Checked by each shell that is
    /// here (`-n`, read without running). Not on Windows, which starts
    /// PowerShell: the `bash` a Windows runner finds fails on them with
    /// nothing said.
    #[cfg(unix)]
    #[test]
    fn each_shell_reads_its_files() {
        let dir = std::env::temp_dir().join(format!("onyx-shell-marks-{}", std::process::id()));
        let files: [(&str, &str, &str); 7] = [
            ("zsh", "zshenv", ZSHENV),
            ("zsh", "zprofile", ZPROFILE),
            ("zsh", "zshrc", ZSHRC),
            ("zsh", "zlogin", ZLOGIN),
            ("zsh", "onyx.zsh", ZSH_MARKS),
            ("bash", "onyx.bash", BASH),
            ("fish", "onyx.fish", FISH),
        ];
        for (shell, name, text) in files {
            let path = dir.join(name);
            put(&path, text).expect("written");
            let Ok(out) = crate::process::command(shell).arg("-n").arg(&path).output() else { continue };
            assert!(out.status.success(), "{shell} -n {name}: {}", String::from_utf8_lossy(&out.stderr));
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
