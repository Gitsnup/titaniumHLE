/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `NSCalendar` and `NSDateComponents`.

use super::ns_date::NSDateHostObject;
use super::{NSInteger, NSTimeInterval, NSUInteger};
use crate::dyld::{ConstantExports, HostConstant};
use crate::frameworks::core_foundation::time::CFAbsoluteTimeGetGregorianDate;
use crate::objc::{id, msg, msg_class, nil, objc_classes, ClassExports, HostObject, NSZonePtr};

const NSUndefinedDateComponent: NSInteger = NSInteger::MAX;

pub const CONSTANTS: ConstantExports =
    &[("_NSGregorianCalendar", HostConstant::NSString("gregorian"))];

#[derive(Clone, Copy)]
struct NSDateComponentsHostObject {
    year: NSInteger,
    month: NSInteger,
    day: NSInteger,
    hour: NSInteger,
    minute: NSInteger,
    second: NSInteger,
}
impl Default for NSDateComponentsHostObject {
    fn default() -> Self {
        Self {
            year: NSUndefinedDateComponent,
            month: NSUndefinedDateComponent,
            day: NSUndefinedDateComponent,
            hour: NSUndefinedDateComponent,
            minute: NSUndefinedDateComponent,
            second: NSUndefinedDateComponent,
        }
    }
}
impl HostObject for NSDateComponentsHostObject {}

#[derive(Default)]
struct NSCalendarHostObject;
impl HostObject for NSCalendarHostObject {}

fn is_leap_year(year: i32) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

fn days_in_month(year: i32, month: i32) -> Option<i32> {
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => return None,
    };
    Some(days)
}

fn days_before_year(year: i32) -> i64 {
    let year = i64::from(year) - 1;
    year * 365 + year / 4 - year / 100 + year / 400
}

fn seconds_since_unix_epoch(components: NSDateComponentsHostObject) -> Option<f64> {
    let year = components.year;
    let month = components.month;
    let day = components.day;
    if !(1..=9999).contains(&year) || !(1..=12).contains(&month) {
        return None;
    }
    let max_day = days_in_month(year, month)?;
    if !(1..=max_day).contains(&day) {
        return None;
    }

    let hour = if components.hour == NSUndefinedDateComponent {
        0
    } else {
        components.hour
    };
    let minute = if components.minute == NSUndefinedDateComponent {
        0
    } else {
        components.minute
    };
    let second = if components.second == NSUndefinedDateComponent {
        0
    } else {
        components.second
    };
    if !(0..=23).contains(&hour) || !(0..=59).contains(&minute) || !(0..=59).contains(&second) {
        return None;
    }

    const DAYS_BEFORE_MONTH: [i32; 12] = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
    let leap_day = i32::from(month > 2 && is_leap_year(year));
    let days = days_before_year(year)
        + i64::from(DAYS_BEFORE_MONTH[(month - 1) as usize] + leap_day + day - 1)
        - days_before_year(1970);
    Some((days * 86_400 + i64::from(hour * 3_600 + minute * 60 + second)) as f64)
}

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

@implementation NSDateComponents: NSObject

+ (id)allocWithZone:(NSZonePtr)_zone {
    env.objc.alloc_object(this, Box::<NSDateComponentsHostObject>::default(), &mut env.mem)
}

- (())dealloc {
    env.objc.dealloc_object(this, &mut env.mem)
}

- (NSInteger)year {
    env.objc.borrow::<NSDateComponentsHostObject>(this).year
}
- (())setYear:(NSInteger)value {
    env.objc.borrow_mut::<NSDateComponentsHostObject>(this).year = value;
}
- (NSInteger)month {
    env.objc.borrow::<NSDateComponentsHostObject>(this).month
}
- (())setMonth:(NSInteger)value {
    env.objc.borrow_mut::<NSDateComponentsHostObject>(this).month = value;
}
- (NSInteger)day {
    env.objc.borrow::<NSDateComponentsHostObject>(this).day
}
- (())setDay:(NSInteger)value {
    env.objc.borrow_mut::<NSDateComponentsHostObject>(this).day = value;
}
- (NSInteger)hour {
    env.objc.borrow::<NSDateComponentsHostObject>(this).hour
}
- (())setHour:(NSInteger)value {
    env.objc.borrow_mut::<NSDateComponentsHostObject>(this).hour = value;
}
- (NSInteger)minute {
    env.objc.borrow::<NSDateComponentsHostObject>(this).minute
}
- (())setMinute:(NSInteger)value {
    env.objc.borrow_mut::<NSDateComponentsHostObject>(this).minute = value;
}
- (NSInteger)second {
    env.objc.borrow::<NSDateComponentsHostObject>(this).second
}
- (())setSecond:(NSInteger)value {
    env.objc.borrow_mut::<NSDateComponentsHostObject>(this).second = value;
}

- (id)copyWithZone:(NSZonePtr)_zone {
    let components = *env.objc.borrow::<NSDateComponentsHostObject>(this);
    env.objc.alloc_object(this, Box::new(components), &mut env.mem)
}

@end

@implementation NSCalendar: NSObject

+ (id)allocWithZone:(NSZonePtr)_zone {
    env.objc.alloc_object(this, Box::<NSCalendarHostObject>::default(), &mut env.mem)
}

+ (id)currentCalendar {
    let calendar: id = msg![env; this alloc];
    msg![env; calendar initWithCalendarIdentifier:nil]
}

- (id)initWithCalendarIdentifier:(id)_identifier { // NSString *
    this
}

- (id)dateFromComponents:(id)components { // NSDateComponents *
    let components = *env.objc.borrow::<NSDateComponentsHostObject>(components);
    let Some(seconds) = seconds_since_unix_epoch(components) else {
        return nil;
    };
    let date: id = msg_class![env; NSDate alloc];
    msg![env; date initWithTimeIntervalSince1970:seconds]
}

- (id)components:(NSUInteger)_units
         fromDate:(id)date { // NSDate *
    let time_interval: NSTimeInterval = env.objc.borrow::<NSDateHostObject>(date).time_interval;
    let gregorian_date = CFAbsoluteTimeGetGregorianDate(env, time_interval, nil);
    let components = NSDateComponentsHostObject {
        year: gregorian_date.year,
        month: gregorian_date.month.into(),
        day: gregorian_date.day.into(),
        hour: gregorian_date.hours.into(),
        minute: gregorian_date.minutes.into(),
        second: gregorian_date.seconds as NSInteger,
    };
    let components_class = env
        .objc
        .get_known_class("NSDateComponents", &mut env.mem);
    env.objc.alloc_object(
        components_class,
        Box::new(components),
        &mut env.mem,
    )
}

@end

};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_gregorian_date_to_unix_seconds() {
        let components = NSDateComponentsHostObject {
            year: 1970,
            month: 1,
            day: 1,
            hour: 0,
            minute: 0,
            second: 0,
        };
        assert_eq!(seconds_since_unix_epoch(components), Some(0.0));

        let components = NSDateComponentsHostObject {
            year: 2000,
            month: 2,
            day: 29,
            hour: 0,
            minute: 0,
            second: 0,
        };
        assert_eq!(seconds_since_unix_epoch(components), Some(951_782_400.0));
    }

    #[test]
    fn rejects_invalid_gregorian_date() {
        let components = NSDateComponentsHostObject {
            year: 2001,
            month: 2,
            day: 29,
            hour: 0,
            minute: 0,
            second: 0,
        };
        assert_eq!(seconds_since_unix_epoch(components), None);
    }
}
