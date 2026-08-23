import { describe, expect, test } from 'bun:test'
import {
	jsonFieldInitialText,
	parseJsonConfigField
} from '../src/components/transforms/transform-schema'

describe('JSON-valued transform config fields', () => {
	test('omits empty optional input instead of treating it as invalid JSON', () => {
		expect(parseJsonConfigField('', false)).toEqual({ kind: 'omit' })
		expect(parseJsonConfigField('   ', false)).toEqual({ kind: 'omit' })
	})

	test('rejects empty required input as invalid JSON', () => {
		expect(parseJsonConfigField('', true)).toEqual({ kind: 'error' })
		expect(parseJsonConfigField('\n', true)).toEqual({ kind: 'error' })
	})

	test('parses quoted JSON and stores JSON null as a present value', () => {
		expect(parseJsonConfigField('"normal"', true)).toEqual({ kind: 'value', value: 'normal' })
		expect(parseJsonConfigField('null', false)).toEqual({ kind: 'value', value: null })
		expect(parseJsonConfigField('true', false)).toEqual({ kind: 'value', value: true })
		expect(parseJsonConfigField('12', false)).toEqual({ kind: 'value', value: 12 })
		expect(parseJsonConfigField('{"a":1}', false)).toEqual({ kind: 'value', value: { a: 1 } })
	})

	test('coerces unquoted tokens to JSON strings', () => {
		expect(parseJsonConfigField('normal', true)).toEqual({ kind: 'value', value: 'normal' })
		expect(parseJsonConfigField('priority', false)).toEqual({ kind: 'value', value: 'priority' })
	})

	test('rejects broken JSON objects, arrays, and string literals', () => {
		expect(parseJsonConfigField('{', false)).toEqual({ kind: 'error' })
		expect(parseJsonConfigField('["a"', false)).toEqual({ kind: 'error' })
		expect(parseJsonConfigField('"unterminated', true)).toEqual({ kind: 'error' })
	})

	test('initializes absent JSON fields as empty text rather than null', () => {
		expect(jsonFieldInitialText({}, 'when_equals')).toBe('')
		expect(jsonFieldInitialText({ when_equals: null }, 'when_equals')).toBe('null')
		expect(jsonFieldInitialText({ value: 'normal' }, 'value')).toBe('"normal"')
	})
})
