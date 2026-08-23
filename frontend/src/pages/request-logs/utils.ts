import type { RequestLog } from '@/lib/api'
import { formatNanoUsd, isSignedIntegerString } from '@/lib/exact-decimal'

type TimingValue = number | string | null | undefined

export type TpsBasis = {
	value: number
	tokens: number
	denominatorMs: number
}

export type ComputedTps =
	| {
			state: 'display'
			/** Wall-clock generation throughput: total output tokens over the generation window (FL4a-1/2). */
			average: TpsBasis | null
			/** Visible-text throughput over the visible generation window (FL4a-5). */
			visible: TpsBasis | null
	  }
	| {
			state: 'unavailable'
	  }

export type BillingValueDimension =
	| 'usageClass'
	| 'unit'
	| 'modality'
	| 'cacheTtl'
	| 'contextTier'
	| 'serviceTier'

const BILLING_VALUE_TRANSLATION_KEYS: Record<
	BillingValueDimension,
	Record<string, string>
> = {
	usageClass: {
		input_uncached: 'requestLogs.billingUsageInputUncached',
		input_cached: 'requestLogs.billingUsageInputCached',
		cache_read: 'requestLogs.billingUsageCacheRead',
		cache_write_5m: 'requestLogs.billingUsageCacheWrite5m',
		cache_write_1h: 'requestLogs.billingUsageCacheWrite1h',
		output: 'requestLogs.billingUsageOutput',
		reasoning_output: 'requestLogs.billingUsageReasoningOutput',
		web_search: 'requestLogs.billingUsageWebSearch',
		file_search_tool_call: 'requestLogs.billingUsageFileSearch',
		x_search: 'requestLogs.billingUsageXSearch',
		code_execution: 'requestLogs.billingUsageCodeExecution',
		code_execution_duration: 'requestLogs.billingUsageCodeExecutionDuration',
		code_interpreter_duration: 'requestLogs.billingUsageCodeExecutionDuration'
	},
	unit: {
		token: 'requestLogs.billingUnitToken',
		call: 'requestLogs.billingUnitCall',
		request: 'requestLogs.billingUnitRequest',
		billed_minute: 'requestLogs.billingUnitBilledMinute'
	},
	modality: {
		text: 'requestLogs.billingModalityText',
		image: 'requestLogs.billingModalityImage',
		audio: 'requestLogs.billingModalityAudio',
		video: 'requestLogs.billingModalityVideo'
	},
	cacheTtl: {
		'5m': 'requestLogs.billingCacheTtl5m',
		'1h': 'requestLogs.billingCacheTtl1h'
	},
	contextTier: {
		default: 'requestLogs.billingTierDefault',
		short: 'requestLogs.billingContextShort',
		long: 'requestLogs.billingContextLong'
	},
	serviceTier: {
		default: 'requestLogs.billingTierDefault',
		standard: 'requestLogs.billingServiceStandard',
		priority: 'requestLogs.billingServicePriority',
		flex: 'requestLogs.billingServiceFlex',
		batch: 'requestLogs.billingServiceBatch'
	}
}

export type JsonObject = Record<string, unknown>

export function asObject(value: unknown): JsonObject | null {
	if (value && typeof value === 'object' && !Array.isArray(value)) {
		return value as JsonObject
	}
	return null
}

export function readNumber(value: unknown): number | null {
	if (typeof value === 'number' && Number.isFinite(value)) return value
	if (typeof value === 'string') {
		const parsed = Number(value)
		return Number.isFinite(parsed) ? parsed : null
	}
	return null
}

export function readTokenCount(obj: JsonObject | null, key: string): number | null {
	if (!obj) return null
	return readNumber(obj[key])
}

export function readNanoString(obj: JsonObject | null, key: string): string | null {
	if (!obj) return null
	const raw = obj[key]
	if (typeof raw === 'string' && raw.trim() !== '') return raw
	return null
}

function parseTimingMs(value: TimingValue): number | null {
	if (typeof value === 'number') {
		return Number.isFinite(value) && value >= 0 ? value : null
	}

	if (typeof value === 'string') {
		const trimmed = value.trim()
		if (!trimmed) return null

		const parsed = Number(trimmed)
		return Number.isFinite(parsed) && parsed >= 0 ? parsed : null
	}

	return null
}

function tpsFromBasis(tokens: number | null, denominatorMs: number | null): TpsBasis | null {
	if (tokens == null || tokens <= 0 || denominatorMs == null || denominatorMs <= 0) {
		return null
	}
	return {
		value: tokens / (denominatorMs / 1000),
		tokens,
		denominatorMs
	}
}

/** FL4a-1: the total output token count for the Average TPS numerator. */
function totalOutputTokens(log: RequestLog): number | null {
	const usageOutput = asObject(asObject(log.usage)?.output)
	return readTokenCount(usageOutput, 'total_tokens') ?? log.tokens.output ?? null
}

function visibleOutputTokens(log: RequestLog): number | null {
	return readNumber(log.timing.visible_output_tokens)
}

export function computeTps(log: RequestLog): ComputedTps {
	const durationMs = getDurationMs(log)
	const ttfbMs = getTtfbMs(log)
	const visibleGenerationMs = parseTimingMs(log.timing.visible_generation_ms)

	// FL4a-2: the Average TPS generation window is the wall-clock span from
	// first upstream chunk to stream end (duration - ttfb), falling back to the
	// full duration when TTFB is unknown.
	const averageWindowMs =
		durationMs != null && ttfbMs != null && durationMs > ttfbMs ?
			durationMs - ttfbMs
		: durationMs

	const outputTotal = totalOutputTokens(log)
	const visibleTokens = visibleOutputTokens(log)

	let average: TpsBasis | null = null
	if (outputTotal != null) {
		average = tpsFromBasis(outputTotal, averageWindowMs)
	} else {
		// FL4a-1: when no output-token total exists, fall back to the visible
		// token count paired with the visible generation window.
		average = tpsFromBasis(visibleTokens, visibleGenerationMs)
	}

	// FL4a-5: the visible-window row is only shown when a visible basis exists.
	const visible =
		outputTotal != null ? tpsFromBasis(visibleTokens, visibleGenerationMs) : null

	if (!average && !visible) {
		return { state: 'unavailable' }
	}
	return { state: 'display', average, visible }
}

export function billingValueTranslationKey(
	dimension: BillingValueDimension,
	value: string
): string | null {
	return BILLING_VALUE_TRANSLATION_KEYS[dimension][value] ?? null
}

export function getDurationMs(log: RequestLog): number | null {
	return parseTimingMs(log.timing.duration_ms)
}

export function getTtfbMs(log: RequestLog): number | null {
	return parseTimingMs(log.timing.ttfb_ms)
}

export function formatCost(nanoUsd: string | null | undefined): string {
	if (nanoUsd == null) return '-'
	if (!isSignedIntegerString(nanoUsd)) return '-'
	return formatNanoUsd(nanoUsd, 6)
}

export function formatDuration(ms: number | null | undefined): string | null {
	if (ms == null) return null
	if (ms < 1000) return `${ms}ms`
	return `${(ms / 1000).toFixed(2)}s`
}

export function formatTime(dateString: string): string {
	const date = new Date(dateString)
	const y = date.getFullYear()
	const mo = String(date.getMonth() + 1).padStart(2, '0')
	const d = String(date.getDate()).padStart(2, '0')
	const h = String(date.getHours()).padStart(2, '0')
	const mi = String(date.getMinutes()).padStart(2, '0')
	const s = String(date.getSeconds()).padStart(2, '0')
	return `${y}-${mo}-${d} ${h}:${mi}:${s}`
}
