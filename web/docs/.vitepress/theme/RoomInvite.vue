<script setup>
import { ref, onMounted, computed } from 'vue'
const link = ref(''), room = ref(''), error = ref(''), copied = ref(false), windows = ref(true), chinese = ref(false)
onMounted(() => {
  windows.value = navigator.platform.startsWith('Win')
  const token = location.hash.slice(1)
  if (!/^v[12]\.[0-9a-f]{2,11976}$/i.test(token) || (token.length - 3) % 2) { error.value = 'Invalid or missing invitation'; return }
  try {
    const bytes = new Uint8Array(token.slice(3).match(/../g).map(x => parseInt(x, 16)))
    const data = JSON.parse(new TextDecoder('utf-8', { fatal: true }).decode(bytes))
    if (typeof data.room !== 'string' || !/^[A-Za-z0-9_-]{1,128}$/.test(data.room)) throw new Error()
    room.value = data.room
    link.value = 'https://frp.sh/join#' + token
    // Keep the secret out of subsequent navigation and the address bar.
    history.replaceState(null, '', location.pathname)
  } catch { error.value = 'Invalid invitation' }
})
const command = computed(() => !link.value ? '' : windows.value
  ? `irm https://frp.sh/install.ps1 | iex; if ($?) { & "$env:ProgramFiles\\frp-sh\\frp-sh.exe" connect '${link.value}' }`
  : `sh -c 's=$(curl -fsSL https://frp.sh/install.sh) && printf "%s\\n" "$s" | sh && exec /usr/local/bin/frp-sh connect "$1"' sh '${link.value}'`)
async function copy() {
  try { await navigator.clipboard.writeText(command.value); copied.value = true; setTimeout(() => copied.value = false, 2000) }
  catch { error.value = chinese.value ? '请选中下方命令手动复制。' : 'Select the command below to copy it manually.' }
}
</script>

<template>
<div class="invite-page">
  <button class="invite-language" @click="chinese = !chinese">{{ chinese ? 'English' : '中文' }}</button>
  <p class="invite-eyebrow">FRP.SH / SHARED SPACE</p>
  <h1>{{ chinese ? '你的朋友在等你。' : 'Your room is waiting.' }}</h1>
  <p class="invite-room" v-if="room">{{ chinese ? '房间' : 'ROOM' }} {{ room }}</p>
  <p>{{ chinese ? '复制一行命令到普通终端：完成安装、保存配置，然后加入房间。' : 'One command installs frp-sh, saves the connection, and joins your room. Run it in your normal terminal.' }}</p>
  <template v-if="link">
    <div class="invite-platform" role="group" aria-label="Operating system">
      <button :aria-pressed="windows" @click="windows = true">Windows / PowerShell</button>
      <button :aria-pressed="!windows" @click="windows = false">macOS / Linux</button>
    </div>
    <button class="invite-copy" @click="copy">{{ copied ? (chinese ? '已复制' : 'Copied') : (chinese ? '复制安装并加入命令' : 'Copy install & join command') }}</button>
    <details><summary>{{ chinese ? '查看完整命令' : 'View full command' }}</summary><pre tabindex="0">{{ command }}</pre></details>
    <p class="invite-note">{{ chinese ? '仅安装阶段需要管理员授权。邀请包含访问凭据，请勿公开分享。需要 frp-sh 0.5.0 或更新版本。' : 'Administrator approval is needed only for installation. This invitation contains access credentials; share it privately. Requires frp-sh 0.5.0 or later.' }}</p>
  </template>
  <p role="status">{{ error }}</p>
</div>
</template>

<style scoped>
.invite-page{max-width:824px;padding:32px;margin:7vh auto 12vh;position:relative}.invite-eyebrow{letter-spacing:.16em;font-size:12px;color:var(--vp-c-text-2)}.invite-page h1{font-size:clamp(36px,6vw,64px);line-height:1.05;letter-spacing:-.04em;margin:32px 0}.invite-room{font-size:28px;font-variant-numeric:tabular-nums}.invite-platform{display:flex;gap:12px;flex-wrap:wrap;margin:32px 0 20px}.invite-platform button{border:1px solid var(--vp-c-divider);border-radius:10px;padding:12px 18px;transition:background .2s,border-color .2s}.invite-platform button[aria-pressed=true]{border-color:var(--vp-c-brand-1);background:var(--vp-c-bg-soft)}.invite-copy{background:var(--vp-c-text-1);color:var(--vp-c-bg);padding:16px 24px;border-radius:12px;font-weight:600;transition:transform .2s}.invite-copy:hover{transform:translateY(-2px)}.invite-note{color:var(--vp-c-text-2);font-size:13px;margin-top:32px}details{margin-top:22px}pre{white-space:pre-wrap;overflow-wrap:anywhere;padding:16px;background:var(--vp-c-bg-soft);border-radius:12px;font-size:12px}.invite-language{position:absolute;right:0;top:0;color:var(--vp-c-text-2)}
</style>
