function wt() {
  local wt_path_file wt_path wt_status wt_cd_status
  wt_path_file=$(mktemp) || return
  WT_SHELL_PATH_FILE="$wt_path_file" command wt "$@"
  wt_status=$?
  if IFS= read -r wt_path < "$wt_path_file" && [[ -n "$wt_path" ]]; then
    builtin cd -- "$wt_path"
    wt_cd_status=$?
    if (( wt_status == 0 && wt_cd_status != 0 )); then
      wt_status=$wt_cd_status
    fi
  fi
  command rm -f -- "$wt_path_file"
  return "$wt_status"
}
