#!/bin/sh
# Axiom Constitution & Integrity Guard (Gate 1~3)
# Called by admin/hooks/pre-commit.

ROOT=$(git rev-parse --show-toplevel) || exit 0
cd "$ROOT" || exit 0
G="git -c core.quotepath=false"
staged=$($G diff --cached --name-only --diff-filter=ACMR)
[ -z "$staged" ] && exit 0
added=$($G diff --cached --name-only --diff-filter=A)
bad=0
say() { echo "[Axiom 헌법 위배] $*" >&2; bad=1; }

# [게이트 1] 파일명 및 경로 검사: ASCII 전용 (한글 파일명 절대 금지)
non_ascii=$(echo "$added" | LC_ALL=C grep -a '[^ -~]')
if [ -n "$non_ascii" ]; then
    say "한글 등 비-ASCII 파일명은 사용할 수 없습니다 (kebab-case 영문 소문자 사용):"
    echo "$non_ascii" | sed 's/^/    /' >&2
fi

# [게이트 2] 비밀 및 키 유출 차단
keyfiles=$(echo "$staged" | grep -Ei '(^|/)(\.env[^/]*|[^/]*\.pem|[^/]*\.p12|[^/]*_api_key\.txt|secrets?)(/|$)')
if [ -n "$keyfiles" ]; then
    say "API 키 또는 비밀 파일로 감지되었습니다. 키는 C:\\Dreamhub\\Project\\.secrets\\ 에 보관하십시오:"
    echo "$keyfiles" | sed 's/^/    /' >&2
fi

key_hits=$($G diff --cached -U0 --diff-filter=ACM | grep -a '^+' | grep -av '^+++' |
    grep -aEi 'sk-[A-Za-z0-9_-]{20,}|AKIA[0-9A-Z]{16}|-----BEGIN [A-Z ]*PRIVATE KEY-----|(api[_-]?key|secret|token|passwd|password)["'"'"' ]*[:=]["'"'"' ]*[A-Za-z0-9_/+=.-]{20,}' | cut -c1-80)
if [ -n "$key_hits" ]; then
    say "커밋 내용에 API 키/패스워드 패턴이 포함되어 있습니다. 공용 키 폴더를 사용하십시오."
fi

# [게이트 3] Axiom-Core 절대 결정론 헌법 (No Float, No External Clock in Core)
core_staged=$($G diff --cached --name-only --diff-filter=ACM | grep -E '^crates/axiom-core/src/.*\.rs$')
if [ -n "$core_staged" ]; then
    # f32, f64 검사
    float_hits=$($G diff --cached -U0 --diff-filter=ACM -- $core_staged | grep -a '^+' | grep -av '^+++' |
        grep -aE '\b(f32|f64)\b')
    if [ -n "$float_hits" ]; then
        say "axiom-core 내부에는 부동소수점(f32, f64)을 사용할 수 없습니다 (절대 결정론 헌법 위배):"
        echo "$float_hits" | head -n 5 | sed 's/^/    /' >&2
    fi

    # SystemTime, Instant 직접 호출 검사
    time_hits=$($G diff --cached -U0 --diff-filter=ACM -- $core_staged | grep -a '^+' | grep -av '^+++' |
        grep -aE '(SystemTime::now|Instant::now)')
    if [ -n "$time_hits" ]; then
        say "axiom-core 내부에는 시스템 실시간 시계(SystemTime, Instant)를 직접 호출할 수 없습니다:"
        echo "$time_hits" | head -n 5 | sed 's/^/    /' >&2
    fi

    # rand 난수 직접 호출 검사
    rand_hits=$($G diff --cached -U0 --diff-filter=ACM -- $core_staged | grep -a '^+' | grep -av '^+++' |
        grep -aE '\brand::(thread_rng|random)\b')
    if [ -n "$rand_hits" ]; then
        say "axiom-core 내부에는 비결정론적 난수(thread_rng, random)를 호출할 수 없습니다 (시드 기반 고정 PRNG 사용):"
        echo "$rand_hits" | head -n 5 | sed 's/^/    /' >&2
    fi
fi

[ "$bad" = 1 ] && exit 1
exit 0
