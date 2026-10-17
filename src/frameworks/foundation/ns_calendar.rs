/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `NSCalendar`.
//!
//! Minimal implementation: only supports the Gregorian calendar, treats all
//! date computations as UTC (like the rest of touchHLE's date code) and
//! ignores unset date components (they are treated as 0).

use super::ns_date_components::NSDateComponentsHostObject;
use super::NSTimeInterval;
use crate::dyld::{ConstantExports, HostConstant};
use crate::objc::{id, msg_class, nil, objc_classes, ClassExports, HostObject, NSZonePtr};

/// TODO: use the real constant value from iOS, if anyone can determine it.
/// Guests do not depend on the literal value; it only matters for equality
/// comparisons against this same constant.
const NS_GREGORIAN_CALENDAR: &str = "com.apple.calendar.gregorian";

#[derive(Default)]
pub(super) struct NSCalendarHostObject {
    identifier: Option<id>,
}
impl HostObject for NSCalendarHostObject {}

pub const CONSTANTS: ConstantExports = &[(
    "_NSGregorianCalendar",
    HostConstant::NSString(NS_GREGORIAN_CALENDAR),
)];

/// Days from 1970-01-01 for a proleptic Gregorian civil date.
/// (Howard Hinnant's `days_from_civil` algorithm.)
fn days_from_unix_epoch(year: i64, month: i64, day: i64) -> i64 {
    let y = year - if month <= 2 { 1 } else { 0 };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// Days from 1970-01-01 to 2001-01-01 (the Apple reference date).
const DAYS_UNIX_TO_APPLE_EPOCH: i64 = 11323;

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

@implementation NSCalendar: NSObject

+ (id)allocWithZone:(NSZonePtr)_zone {
    let host_object = Box::<NSCalendarHostObject>::default();
    env.objc.alloc_object(this, host_object, &mut env.mem)
}

- (id)init {
    let gregorian = super::ns_string::get_static_str(env, NS_GREGORIAN_CALENDAR);
    env.objc.borrow_mut::<NSCalendarHostObject>(this).identifier = Some(gregorian);
    this
}

- (id)initWithCalendarIdentifier:(id)identifier {
    // TODO: other calendar identifiers. Guests that only use the Gregorian
    // calendar get correct behavior.
    env.objc.borrow_mut::<NSCalendarHostObject>(this).identifier = Some(identifier);
    this
}

- (id)calendarIdentifier {
    env.objc.borrow::<NSCalendarHostObject>(this).identifier.unwrap_or(nil)
}

// TODO: also implement components:fromDate:, dateByAddingComponents:toDate:,
// etc, if a guest needs them.

- (id)dateFromComponents:(id)components {
    let &NSDateComponentsHostObject {
        era: _,
        year,
        month,
        day,
        hour,
        minute,
        second,
        nanosecond,
        weekday: _,
        weekday_ordinal: _,
        quarter: _,
        week_of_month: _,
        week_of_year: _,
        year_for_week_of_year: _,
    } = env.objc.borrow::<NSDateComponentsHostObject>(components);

    let year = year.unwrap_or(0) as i64;
    let month = month.unwrap_or(0) as i64;
    let day = day.unwrap_or(0) as i64;
    let hour = hour.unwrap_or(0) as i64;
    let minute = minute.unwrap_or(0) as i64;
    let second = second.unwrap_or(0) as i64;
    let nanosecond = nanosecond.unwrap_or(0) as f64;

    let days = days_from_unix_epoch(year, month, day) - DAYS_UNIX_TO_APPLE_EPOCH;
    let secs: NSTimeInterval =
        (days * 86400 + hour * 3600 + minute * 60 + second) as f64 + nanosecond / 1_000_000_000.0;

    msg_class![env; NSDate dateWithTimeIntervalSinceReferenceDate: secs]
}

@end

};
