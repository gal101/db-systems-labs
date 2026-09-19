use crate::kwaymerge::KwayPage;
use crate::{PageID, RecordID, SlotID};
use std::iter::zip;
use crate::kwaymerge::basic_tests_kwmerge::is_sorted;

/// Sorts the input pages leveraging a k-way merge algorithm.
///
/// It is expected that this function makes use of the 'merge' function.
/// `input_pages` and `output_pages` will always have the same length and will never be empty.
/// The single pages within `input_pages` will also never be empty, but will not be necessarily
/// filled completely (page.len() != KwayPage::CAPACITY).
///
/// Note: `input_pages` and `output_pages` are empty if their length is 0 (will never happen).
/// A KwayPage, instead, is considered empty if its `length` attribute is 0, regardless of its
/// actual content.
///
///
/// # Arguments
///
/// * `k`: Arity of the merge operation (k buffers hold the pages to merge, and one extra buffer holds
/// the output of the merge process).
/// * `input_pages`: Slice containing the pages to sort.
/// * `output_pages`: Slice containing the sorted pages.
pub fn sort(k: usize, input_pages: &[KwayPage], output_pages: &mut [KwayPage]) {
    // To help you better understand how this function should behave, we will use the following
    // running example:
    // k = 3
    // input_pages = [5 4 1 7 15 4 9 12 8 2 6 5]
    // output_pages = [0 0 0 0 0 0 0 0 0 0 0 0]
    //
    // This example simplify things by using integers instead of pages. In reality,
    // `input_pages` and `output_pages` will contain elements of type KwayPage.

    // Base case, trivially sort a single page.
    if input_pages.len() == 1 {
        let page = &input_pages[0];
        let mut vec = page.records[0 ..page.length].to_vec();
        vec.sort();

        for (index, elem) in vec.iter().enumerate() {
            output_pages[0].update(*elem, index).expect("Database error");
        }

        output_pages[0].length = page.length;
        return;
    }

    // `stepsize` represents the distance in the `input_pages` slice between the k
    // lists that need to be merged.
    // Running example:
    // 3-way merge where `input_pages` contains 12 pages.
    // The 3 lists to merge start respectively at index 0, 4 and 8.
    // Input_pages: [5 4 1 7 15 4 9 12 8 2 6 5]
    // Seperation of lists to merge: [(5 4 1 7) (15 4 9 12) (8 2 6 5)]
    // Hint: you cannot call the 'merge' process on these sublists yet, as they are not sorted!
    // However, recursion helps a lot in these situations.
    // If `stepsize` is not the result of a perfect divison the "extra" elements will
    // fall in the last of the k lists.
    // Example:
    // input_pages.len() = 14
    // k = 3
    // stepsize = 14/3 = 4
    // separation of lists to merge: [(x x x x) (x x x x) (x x x x x x)]
    let stepsize = input_pages.len() / k;

    // A vector of size `k` representing the indices of the current pages in the `input_pages`
    // slice that are being merged together (one entry for each list).
    // They change during the merge process when a page is exhausted and the next page
    // from the same list needs to be loaded.
    // Running example:
    // pages_to_merge = [0 4 8], which are the indices of the first page of each list being merged.
    // These indexes increment during the merging process (see `merge` function).
    // When a list is exhausted (all of its pages have been merged), its
    // corresponding entry in `pages_to_merge` will be None.
    let mut pages_to_merge: Vec<Option<usize>> = vec![];

    // You are now expected to:
    // 1) Split the `input_pages` slice in k lists using `stepsize` and `pages_to_merge`.
    //    Note: the "splitting" process does not need any support vector, it can be done by
    //    keeping track of the indices using `pages_to_merge`.
    // 2.1) If the k lists are sorted, merge them using the `merge` function.
    // 2.2) If the k lists are not sorted, recursively sort them with a mix of `sort` and `merge`.
    // 3) Write the final, sorted list in the `output_pages` slice.
    //
    // Hint: It is not relevant the order in which you merge the lists together (left-to-right or
    // right-to-left), as long as `output_pages` contains the sorted result. However, we recommend
    // a left-to-right approach.

    let mut sorted_pages = input_pages.to_vec();

    if stepsize == 0 {


        for i in 0..input_pages.len() {
            pages_to_merge.push(Some(i));
            if !is_sorted(Vec::from(&input_pages[i..i+1])) {
                sort(k, &input_pages[i..i+1], &mut sorted_pages[i..i+1]);
            }
        }
        
        merge(&mut pages_to_merge, 1, &*sorted_pages, output_pages);
        return;
    }

    for i in 0..k {
        pages_to_merge.push(Some(i * stepsize));
    }

    for i in 0..k {
        let start_index = pages_to_merge[i].unwrap();
        let end_index = if i < k - 1 { (i+1) * stepsize } else { input_pages.len() };
        if !is_sorted(Vec::from(&input_pages[start_index..end_index])) {

            sort(k, &input_pages[start_index..end_index], &mut sorted_pages[start_index..end_index])
        }
    }
    merge(&mut pages_to_merge, stepsize, &*sorted_pages, output_pages);
}

/// Merges k sorted lists of arbitrary size (i.e., arbitrary number of pages in each list).
///
/// `input_pages` and `output_pages` will always have the same length and will never be empty.
///
/// # Arguments
///
/// * `pages_to_merge`: A vector of size 'k' representing the indices of the current pages that are
///   being merged (one entry for each list). See `sort` function for a more detailed description.
/// * `stepsize`: the distance in the `input_pages` slice between the k lists that need to be merged.
///   See `sort` function for a more detailed description.
/// * `input_pages`: A vector of pages representing the k lists to merge.
/// * `output_pages`: A vector with the same number of pages as 'input_pages' that will contain the sorted
///   output pages.
///
/// returns: Current RecordIDs (PageID + SlotID) for each of the k pages that are being merged when
///   an output page becomes full and needs to be substituted by a new one.
///
/// # Example
///
/// 3-way merge of the following `input_pages` vector:
/// input_pages = [PageID-0 PageID-1 PageID-2 PageID-3 PageID-4 PageID-5 PageID-6 PageID-7 PageID-8]
/// input_pages.len() = 9
/// pages_to_merge = [0, 3, 6] (which are the indices of PageID-0, PageID-3 and PageID-6). Do not
/// confuse the indices with the IDs of the pages: input_pages[0] now contains PageID-0, but it
/// could've also contained pageID-100!
/// Each page contains page.len() number of records.
///
/// Note: The pages of each list are sorted! Meaning that if we take PageID-0, PageID-1 and PageID-2
/// and we read them sequentially we will find all records sorted not only intra-page,
/// but also across all pages!
///
/// 1) PageID 0: [2 7 8 9]
///                 ^
/// 2) PageID 3: [0 6 7 10]
///                 ^
/// 3) PageID 6: [1 5 8 12]
///                   ^
/// The current states are:
/// (PageID 0, SlotID 1)
/// (PageID 3, SlotID 1)
/// (PageID 6, SlotID 2)
///
/// Assume that adding the value 6 from PageID-3 to the output page resulted in
/// outpage.len() == KwayPage::CAPACITY. Thus, the output page needs to be swapped for a new one.
/// The state to be inserted into the output vector in this case is the one reported above. After
/// that, the SlotID of PageID 3 can be increased by 1.
///
/// # Hints
/// 1) If during the selection of the mimimum record when merging there are duplicates, choose the first
///    occurrence of the value (e.g., merging pages 0 1 and 2 -> pages 0 and 1 have the same record X
///    as a candidate minimum -> pick the value from page 0).
/// 2) After writing a record to the output page, check its length. If it became full (and thus
///    needs to be swapped), use the current slots from each of the k page to populate the
///    resulting Vec<RecordID> and THEN increase the slot of the page that contains the minimum
///    record.
pub fn merge(
    pages_to_merge: &mut Vec<Option<usize>>,
    mut stepsize: usize,
    input_pages: &[KwayPage],
    output_pages: &mut [KwayPage],
) -> Vec<RecordID> {
    //Output vector containing the state of k pages when a given output page was finalized.
    let mut states_at_finalizing: Vec<RecordID> = vec![];

    // The number of lists to merge together ('k-way' merge).
    let k = pages_to_merge.len();

    // A vector of size 'k' representing the current slots for each page in 'pages_to_marge'.
    // curr_slots[0] will contain the current slot (the one you need to check during the merge process)
    // of pages_to_merge[0], which is the current active page of the first of the 'k' lists.
    // You should increase it after a value in a page is selected as the minimum during the merge process.
    let mut curr_slots = vec![0; k];

    // Variable to keep track of the current page in `output_pages` being written.
    let mut outpage_idx: usize = 0;

    // You are now expected to:
    // 1) Iterate over the current active page for each list (see 'pages_to_merge').
    // 2) Choose the minimum value among them (see 'curr_slots').
    // 3) Update the output_pages vector with the minimum.
    // 4) When necessary, update the 'state_at_finalizing' vector.

    let mut index_out_page: usize = 0;
    let mut lists_start = vec![];
    for i in 0..k {
        lists_start.push(i * stepsize);
    }
    lists_start.push(input_pages.len());

    'mainloop: loop {
        let mut min_page: i32 = -1;
        let mut min_elem: i32 = -1;
        'lists: for i in 0..k {

            if let Some(page_index) = pages_to_merge[i] {

                if min_page == -1 {
                    min_page = i as i32;
                    min_elem = input_pages[page_index as usize].records[curr_slots[min_page as usize]];
                    continue 'lists;
                }

                let curr_elem = input_pages[page_index].records[curr_slots[i]];
                if curr_elem < min_elem {
                    min_page = i as i32;
                    min_elem = curr_elem;
                }
            }
        }

        if min_page == -1 {
            break 'mainloop;
        }
        
        let min_page = min_page as usize;

        let out_page = &mut output_pages[outpage_idx];
        out_page.records[index_out_page] = min_elem;
        index_out_page += 1;
        out_page.length = index_out_page;

        //OUT PAGE IS FULL
        if index_out_page == out_page.records.len() {
            //add states
            for i in 0..k {
                let page = pages_to_merge[i];

                if let Some(page_index) = page {
                    states_at_finalizing.push(RecordID::new(input_pages[page_index].id, SlotID::from(curr_slots[i] as u16)))
                }
            }
            
            outpage_idx += 1;
            index_out_page = 0;
            
            if outpage_idx == output_pages.len() {
                break 'mainloop;
            }

            output_pages[outpage_idx].length = 0;
        }
        
        curr_slots[min_page] += 1;
        let mut page_index_input = pages_to_merge[min_page].unwrap();
        let actual_min_page = &input_pages[page_index_input];
        if curr_slots[min_page] == actual_min_page.length {
            
            page_index_input += 1;
            if page_index_input == lists_start[min_page + 1] || input_pages[page_index_input].length == 0 {
                pages_to_merge[min_page] = None;
            } else {
                pages_to_merge[min_page] = Some(page_index_input);
                curr_slots[min_page] = 0;
            }
        }

    }

    for i in outpage_idx + 1..output_pages.len() {
        output_pages[i].length = 0;
    }

    states_at_finalizing
}