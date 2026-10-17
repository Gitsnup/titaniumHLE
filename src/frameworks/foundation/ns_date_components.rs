/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `NSDateComponents`.

use super::{NSInteger, NSUndefinedDateComponent};
use crate::objc::{id, objc_classes, ClassExports, HostObject, NSZonePtr};

#[derive(Default)]
pub(super) struct NSDateComponentsHostObject {
    pub(super) era: Option<NSInteger>,
    pub(super) year: Option<NSInteger>,
    pub(super) month: Option<NSInteger>,
    pub(super) day: Option<NSInteger>,
    pub(super) hour: Option<NSInteger>,
    pub(super) minute: Option<NSInteger>,
    pub(super) second: Option<NSInteger>,
    pub(super) nanosecond: Option<NSInteger>,
    pub(super) weekday: Option<NSInteger>,
    pub(super) weekday_ordinal: Option<NSInteger>,
    pub(super) quarter: Option<NSInteger>,
    pub(super) week_of_month: Option<NSInteger>,
    pub(super) week_of_year: Option<NSInteger>,
    pub(super) year_for_week_of_year: Option<NSInteger>,
}
impl HostObject for NSDateComponentsHostObject {}

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

@implementation NSDateComponents: NSObject

+ (id)allocWithZone:(NSZonePtr)_zone {
    let host_object = Box::<NSDateComponentsHostObject>::default();
    env.objc.alloc_object(this, host_object, &mut env.mem)
}

- (())setEra:(NSInteger)era {
    env.objc.borrow_mut::<NSDateComponentsHostObject>(this).era = Some(era);
}
- (())setYear:(NSInteger)year {
    env.objc.borrow_mut::<NSDateComponentsHostObject>(this).year = Some(year);
}
- (())setMonth:(NSInteger)month {
    env.objc.borrow_mut::<NSDateComponentsHostObject>(this).month = Some(month);
}
- (())setDay:(NSInteger)day {
    env.objc.borrow_mut::<NSDateComponentsHostObject>(this).day = Some(day);
}
- (())setHour:(NSInteger)hour {
    env.objc.borrow_mut::<NSDateComponentsHostObject>(this).hour = Some(hour);
}
- (())setMinute:(NSInteger)minute {
    env.objc.borrow_mut::<NSDateComponentsHostObject>(this).minute = Some(minute);
}
- (())setSecond:(NSInteger)second {
    env.objc.borrow_mut::<NSDateComponentsHostObject>(this).second = Some(second);
}
- (())setNanosecond:(NSInteger)nanosecond {
    env.objc.borrow_mut::<NSDateComponentsHostObject>(this).nanosecond = Some(nanosecond);
}
- (())setWeekday:(NSInteger)weekday {
    env.objc.borrow_mut::<NSDateComponentsHostObject>(this).weekday = Some(weekday);
}
- (())setWeekdayOrdinal:(NSInteger)weekday_ordinal {
    env.objc.borrow_mut::<NSDateComponentsHostObject>(this).weekday_ordinal = Some(weekday_ordinal);
}
- (())setQuarter:(NSInteger)quarter {
    env.objc.borrow_mut::<NSDateComponentsHostObject>(this).quarter = Some(quarter);
}
- (())setWeekOfMonth:(NSInteger)week_of_month {
    env.objc.borrow_mut::<NSDateComponentsHostObject>(this).week_of_month = Some(week_of_month);
}
- (())setWeekOfYear:(NSInteger)week_of_year {
    env.objc.borrow_mut::<NSDateComponentsHostObject>(this).week_of_year = Some(week_of_year);
}
- (())setYearForWeekOfYear:(NSInteger)year_for_week_of_year {
    env.objc.borrow_mut::<NSDateComponentsHostObject>(this).year_for_week_of_year = Some(year_for_week_of_year);
}

- (NSInteger)era {
    env.objc.borrow::<NSDateComponentsHostObject>(this).era.unwrap_or(NSUndefinedDateComponent)
}
- (NSInteger)year {
    env.objc.borrow::<NSDateComponentsHostObject>(this).year.unwrap_or(NSUndefinedDateComponent)
}
- (NSInteger)month {
    env.objc.borrow::<NSDateComponentsHostObject>(this).month.unwrap_or(NSUndefinedDateComponent)
}
- (NSInteger)day {
    env.objc.borrow::<NSDateComponentsHostObject>(this).day.unwrap_or(NSUndefinedDateComponent)
}
- (NSInteger)hour {
    env.objc.borrow::<NSDateComponentsHostObject>(this).hour.unwrap_or(NSUndefinedDateComponent)
}
- (NSInteger)minute {
    env.objc.borrow::<NSDateComponentsHostObject>(this).minute.unwrap_or(NSUndefinedDateComponent)
}
- (NSInteger)second {
    env.objc.borrow::<NSDateComponentsHostObject>(this).second.unwrap_or(NSUndefinedDateComponent)
}
- (NSInteger)nanosecond {
    env.objc.borrow::<NSDateComponentsHostObject>(this).nanosecond.unwrap_or(NSUndefinedDateComponent)
}
- (NSInteger)weekday {
    env.objc.borrow::<NSDateComponentsHostObject>(this).weekday.unwrap_or(NSUndefinedDateComponent)
}
- (NSInteger)weekdayOrdinal {
    env.objc.borrow::<NSDateComponentsHostObject>(this).weekday_ordinal.unwrap_or(NSUndefinedDateComponent)
}
- (NSInteger)quarter {
    env.objc.borrow::<NSDateComponentsHostObject>(this).quarter.unwrap_or(NSUndefinedDateComponent)
}
- (NSInteger)weekOfMonth {
    env.objc.borrow::<NSDateComponentsHostObject>(this).week_of_month.unwrap_or(NSUndefinedDateComponent)
}
- (NSInteger)weekOfYear {
    env.objc.borrow::<NSDateComponentsHostObject>(this).week_of_year.unwrap_or(NSUndefinedDateComponent)
}
- (NSInteger)yearForWeekOfYear {
    env.objc.borrow::<NSDateComponentsHostObject>(this).year_for_week_of_year.unwrap_or(NSUndefinedDateComponent)
}

@end

};
