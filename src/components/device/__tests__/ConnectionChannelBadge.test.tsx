import '@testing-library/jest-dom/vitest'
import { render, screen, cleanup } from '@testing-library/react'
import { afterEach, beforeAll, describe, expect, it } from 'vitest'
import { deriveBadgeKind } from '@/components/device/connection-channel-utils'
import ConnectionChannelBadge from '@/components/device/ConnectionChannelBadge'
import i18n from '@/i18n'

beforeAll(async () => {
  await i18n.changeLanguage('zh-CN')
})

afterEach(() => {
  cleanup()
})

describe('deriveBadgeKind — Phase 96 INDIC-01 truth-table', () => {
  it('direct ⇒ lan,无关 LAN-only 设置', () => {
    expect(deriveBadgeKind('direct', false)).toBe('lan')
    expect(deriveBadgeKind('direct', true)).toBe('lan')
  })

  it('relay + LAN-only OFF ⇒ relay', () => {
    expect(deriveBadgeKind('relay', false)).toBe('relay')
  })

  it('relay + LAN-only ON ⇒ outOfLan(灰态)', () => {
    // INDIC-03 核心:开启 LAN-only 后跨网段经 relay 的设备应转为 outOfLan
    expect(deriveBadgeKind('relay', true)).toBe('outOfLan')
  })

  it('offline + LAN-only OFF ⇒ offline', () => {
    expect(deriveBadgeKind('offline', false)).toBe('offline')
  })

  it('offline + LAN-only ON ⇒ outOfLan(灰态)', () => {
    expect(deriveBadgeKind('offline', true)).toBe('outOfLan')
  })

  it('unknown ⇒ unknown,永远不被合成为其他态(Pitfall 4)', () => {
    expect(deriveBadgeKind('unknown', false)).toBe('unknown')
    expect(deriveBadgeKind('unknown', true)).toBe('unknown')
  })
})

describe('ConnectionChannelBadge — render', () => {
  it('direct + LAN-only OFF 渲染"直连"标签', () => {
    render(<ConnectionChannelBadge channel="direct" lanOnlyActive={false} />)
    expect(screen.getByText('直连')).toBeInTheDocument()
  })

  it('relay + LAN-only ON 渲染"不在本地网络"标签(合成态)', () => {
    render(<ConnectionChannelBadge channel="relay" lanOnlyActive={true} />)
    expect(screen.getByText('不在本地网络')).toBeInTheDocument()
  })

  it('relay + LAN-only OFF 渲染"中转"标签', () => {
    render(<ConnectionChannelBadge channel="relay" lanOnlyActive={false} />)
    expect(screen.getByText('中转')).toBeInTheDocument()
  })

  it('unknown 永远渲染 — 占位符', () => {
    render(<ConnectionChannelBadge channel="unknown" lanOnlyActive={true} />)
    expect(screen.getByText('—')).toBeInTheDocument()
  })
})
