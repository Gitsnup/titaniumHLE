/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `NSDecimalNumber`.
//!
//! Internally, decimal values are approximated with a double, which is plenty
//! for the use cases (e.g. analytics SDKs logging prices) seen so far.

use super::ns_string;
use super::ns_value::NSNumberHostObject;
use super::{
    NSComparisonResult, NSOrderedAscending, NSOrderedDescending, NSOrderedSame, NSUInteger,
};
use crate::objc::{
    autorelease, id, msg, msg_class, objc_classes, release, retain, ClassExports, NSZonePtr,
};

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

@implementation NSDecimalNumber: NSNumber

+ (id)allocWithZone:(NSZonePtr)_zone {
    let host_object = Box::new(NSNumberHostObject::Double(0.0));
    env.objc.alloc_object(this, host_object, &mut env.mem)
}

+ (id)decimalNumberWithMantissa:(i64)mantissa
                        exponent:(i16)exponent
                      isNegative:(bool)is_negative {
    let value = (mantissa as f64)
        * 10f64.powi(exponent as i32)
        * if is_negative { -1.0 } else { 1.0 };
    let new: id = msg![env; this alloc];
    let new: id = msg![env; new initWithDouble:value];
    autorelease(env, new)
}

+ (id)decimalNumberWithDouble:(f64)value {
    let new: id = msg![env; this alloc];
    let new: id = msg![env; new initWithDouble:value];
    autorelease(env, new)
}

+ (id)decimalNumberWithString:(id)string { // NSString *
    let rust_string = ns_string::to_rust_string(env, string).to_string();
    let value: f64 = rust_string.trim().parse().unwrap_or_else(|_| {
        // NSDecimalNumber parsing is much more lenient than Rust's f64
        // parser. Fall back to scanning the leading numeric portion.
        let mut end = 0;
        for (i, c) in rust_string.char_indices() {
            if c.is_ascii_digit() || c == '.' || c == '-' || c == '+' || c == 'e' || c == 'E' {
                end = i + c.len_utf8();
            } else if i > 0 || c != ' ' {
                break;
            }
        }
        rust_string[..end].parse().unwrap_or(0.0)
    });
    let new: id = msg![env; this alloc];
    let new: id = msg![env; new initWithDouble:value];
    autorelease(env, new)
}

+ (id)zero {
    msg![env; this decimalNumberWithDouble:0.0]
}

+ (id)one {
    msg![env; this decimalNumberWithDouble:1.0]
}

+ (id)minimumDecimalNumber {
    let min = f64::MIN;
    msg![env; this decimalNumberWithDouble:min]
}

+ (id)maximumDecimalNumber {
    let max = f64::MAX;
    msg![env; this decimalNumberWithDouble:max]
}

+ (id)notANumber {
    let nan = f64::NAN;
    let new: id = msg![env; this alloc];
    let new: id = msg![env; new initWithDouble:nan];
    autorelease(env, new)
}

- (id)initWithMantissa:(i64)mantissa
               exponent:(i16)exponent
             isNegative:(bool)is_negative {
    let value = (mantissa as f64)
        * 10f64.powi(exponent as i32)
        * if is_negative { -1.0 } else { 1.0 };
    msg![env; this initWithDouble:value]
}

- (id)initWithString:(id)string { // NSString *
    let new: id = msg_class![env; NSDecimalNumber decimalNumberWithString:string];
    retain(env, new);
    release(env, this);
    new
}

- (id)initWithDecimal:(id)decimal {
    // TODO: NSDecimal is not modeled yet; treat as double.
    log!("TODO: [(NSDecimalNumber*) {:?} initWithDecimal:{:?}] (approximating)", this, decimal);
    msg![env; this initWithDouble:0.0]
}

- (id)decimalNumberByAdding:(id)decimal_number {
    let a: f64 = msg![env; this doubleValue];
    let b: f64 = msg![env; decimal_number doubleValue];
    msg_class![env; NSDecimalNumber decimalNumberWithDouble:(a + b)]
}

- (id)decimalNumberBySubtracting:(id)decimal_number {
    let a: f64 = msg![env; this doubleValue];
    let b: f64 = msg![env; decimal_number doubleValue];
    msg_class![env; NSDecimalNumber decimalNumberWithDouble:(a - b)]
}

- (id)decimalNumberByMultiplyingBy:(id)decimal_number {
    let a: f64 = msg![env; this doubleValue];
    let b: f64 = msg![env; decimal_number doubleValue];
    msg_class![env; NSDecimalNumber decimalNumberWithDouble:(a * b)]
}

- (id)decimalNumberByDividingBy:(id)decimal_number {
    let a: f64 = msg![env; this doubleValue];
    let b: f64 = msg![env; decimal_number doubleValue];
    msg_class![env; NSDecimalNumber decimalNumberWithDouble:(a / b)]
}

- (id)decimalNumberByRaisingToPower:(NSUInteger)power {
    let a: f64 = msg![env; this doubleValue];
    msg_class![env; NSDecimalNumber decimalNumberWithDouble:(a.powi(power as i32))]
}

- (id)decimalNumberByRoundingAccordingToBehavior:(id)behavior {
    // TODO: rounding behaviors (NSRoundUp etc.)
    log!("TODO: [(NSDecimalNumber*) {:?} decimalNumberByRoundingAccordingToBehavior:{:?}] (ignoring)", this, behavior);
    retain(env, this)
}

- (NSComparisonResult)compare:(id)other {
    let a: f64 = msg![env; this doubleValue];
    let b: f64 = msg![env; other doubleValue];
    a.partial_cmp(&b).map_or(
        NSOrderedSame, // NaN comparison is not meaningful here
        |ord| match ord {
            std::cmp::Ordering::Less => NSOrderedAscending,
            std::cmp::Ordering::Equal => NSOrderedSame,
            std::cmp::Ordering::Greater => NSOrderedDescending,
        },
    )
}

@end

};
