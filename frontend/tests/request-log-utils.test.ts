import { describe, expect, test } from 'bun:test'
import type { RequestLog } from '../src/lib/api'
import en from '../src/locales/en.json'
import ja from '../src/locales/ja.json'
import zh from '../src/locales/zh.json'
import zhTw from '../src/locales/zh-TW.json'
import {
	billingValueTranslationKey,
	computeTps,
	formatCost,
	type BillingValueDimension
} from '../src/pages/request-logs/utils'

function requestLog(overrides: Partial<RequestLog> = {}): RequestLog {
	return {
		id: 'log-1',
		created_at: '2026-07-18T00:00:00.000Z',
		status: 'success',
		is_stream: true,
		model: 'test-model',
		provider: {},
		channel: {},
		user: { id: 'user-1' },
		api_key: {},
		tokens: {},
		timing: {},
		billing: {},
		error: {},
		...overrides
	}
}

describe('computeTps', () => {
	test('pairs the total output numerator with the wall-clock generation window', () => {
		const result = computeTps(
			requestLog({
				tokens: { output: 30 },
				timing: { duration_ms: 1200, ttfb_ms: 200 }
			})
		)

		expect(result.state).toBe('display')
		if (result.state === 'display') {
			expect(result.average).toEqual({ value: 30, tokens: 30, denominatorMs: 1000 })
			expect(result.visible).toBeNull()
		}
	})

	test('shows the visible-window TPS beside the average when a visible basis exists', () => {
		const result = computeTps(
			requestLog({
				tokens: { output: 30 },
				timing: {
					duration_ms: 1200,
					ttfb_ms: 200,
					visible_output_tokens: 12,
					visible_generation_ms: 500
				}
			})
		)

		expect(result.state).toBe('display')
		if (result.state === 'display') {
			expect(result.average).toEqual({ value: 30, tokens: 30, denominatorMs: 1000 })
			expect(result.visible).toEqual({ value: 24, tokens: 12, denominatorMs: 500 })
		}
	})

	test('falls back to the visible basis when no output total exists', () => {
		const result = computeTps(
			requestLog({
				timing: { visible_output_tokens: 4, visible_generation_ms: 250 }
			})
		)

		expect(result.state).toBe('display')
		if (result.state === 'display') {
			expect(result.average).toEqual({ value: 16, tokens: 4, denominatorMs: 250 })
			expect(result.visible).toBeNull()
		}
	})

	test('uses duration when TTFB is absent', () => {
		const result = computeTps(
			requestLog({
				tokens: { output: 12 },
				timing: { duration_ms: 900 }
			})
		)

		expect(result.state).toBe('display')
		if (result.state === 'display') {
			expect(result.average).toEqual({ value: 12 / 0.9, tokens: 12, denominatorMs: 900 })
		}
	})

	test('omits TPS when no positive token numerator exists', () => {
		expect(
			computeTps(
				requestLog({
					tokens: { output: 0 },
					timing: { duration_ms: 500, visible_output_tokens: 0 }
				})
			)
		).toEqual({ state: 'unavailable' })
	})
})

describe('formatCost', () => {
	test('formats nano-USD with exactly six digits and exact half-up rounding', () => {
		expect(formatCost('123000')).toBe('$0.000123')
		expect(formatCost('123499')).toBe('$0.000123')
		expect(formatCost('123500')).toBe('$0.000124')
	})

	test('does not narrow large integer strings through Number', () => {
		expect(formatCost('9007199254740993000')).toBe('$9,007,199,254.740993')
	})
})

describe('billing breakdown translations', () => {
	const canonicalValues: Array<[BillingValueDimension, string]> = [
		['usageClass', 'input_uncached'],
		['usageClass', 'cache_read'],
		['usageClass', 'cache_write_5m'],
		['usageClass', 'cache_write_1h'],
		['usageClass', 'output'],
		['usageClass', 'reasoning_output'],
		['usageClass', 'web_search'],
		['usageClass', 'file_search_tool_call'],
		['usageClass', 'x_search'],
		['usageClass', 'code_execution'],
		['usageClass', 'code_execution_duration'],
		['usageClass', 'code_interpreter_duration'],
		['unit', 'token'],
		['unit', 'call'],
		['unit', 'request'],
		['unit', 'billed_minute'],
		['modality', 'text'],
		['modality', 'image'],
		['modality', 'audio'],
		['modality', 'video'],
		['cacheTtl', '5m'],
		['cacheTtl', '1h'],
		['contextTier', 'default'],
		['contextTier', 'short'],
		['contextTier', 'long'],
		['serviceTier', 'default'],
		['serviceTier', 'standard'],
		['serviceTier', 'priority'],
		['serviceTier', 'flex'],
		['serviceTier', 'batch']
	]
	const staticKeys = [
		'billingUnitGeneric',
		'billingModality',
		'billingCacheTtl',
		'tpsGenerationWindow'
	] as const
	const locales = [en, zh, zhTw, ja]

	test('every canonical value resolves in every shipped locale', () => {
		for (const [dimension, value] of canonicalValues) {
			const key = billingValueTranslationKey(dimension, value)
			expect(key).not.toBeNull()
			const requestLogsKey = key?.replace('requestLogs.', '')
			for (const locale of locales) {
				expect(locale.requestLogs[requestLogsKey as keyof typeof locale.requestLogs]).toBeTruthy()
			}
		}
		for (const locale of locales) {
			for (const key of staticKeys) expect(locale.requestLogs[key]).toBeTruthy()
		}
	})

	test('preserves unknown custom profile values by returning no translation key', () => {
		expect(billingValueTranslationKey('usageClass', 'custom_gpu_second')).toBeNull()
	})
})
