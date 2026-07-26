import _ from 'lodash';

export function groupByCategory(items) {
    return _.groupBy(items, 'category');
}